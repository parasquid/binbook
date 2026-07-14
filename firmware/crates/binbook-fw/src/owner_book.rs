use binbook_core::{
    BookIndex, BookIndexHeader, FormatError, PageInfo, ReadRange, RenditionIdentity,
};

use crate::storage_owner::{
    BufferSlot, Request, RequestEnvelope, Response, ResponseEnvelope, StorageClient, StorageName,
    MAX_BOUNDED_IO_BYTES,
};
use xteink_x4_display::page_source::{PlaneDecodeStep, StreamingPlaneDecoder};

const HEADER_BYTES: usize = binbook_core::HEADER_SIZE;

#[derive(Debug, Clone)]
pub struct OwnerBook {
    name: StorageName,
    index: BookIndex,
    rendition_identity: RenditionIdentity,
}

impl OwnerBook {
    #[must_use]
    pub const fn name(&self) -> &StorageName {
        &self.name
    }

    #[must_use]
    pub const fn index(&self) -> BookIndex {
        self.index
    }

    #[must_use]
    pub const fn page_count(&self) -> u32 {
        self.index.page_count()
    }

    #[must_use]
    pub const fn rendition_identity(&self) -> RenditionIdentity {
        self.rendition_identity
    }
}

/// Decide whether retained book metadata and page metadata exactly describe a
/// requested reopen. Hash, rendition, and page must all match; a partial match
/// is never sufficient to reuse cached offsets.
#[must_use]
pub fn can_reuse_warm_open(
    selected_filename_hash: u64,
    cached_filename_hash: u64,
    expected_rendition_hash: &[u8; 32],
    cached_rendition_hash: &[u8; 32],
    saved_page: u32,
    cached_page: u32,
) -> bool {
    selected_filename_hash == cached_filename_hash
        && expected_rendition_hash == cached_rendition_hash
        && saved_page == cached_page
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerBookOpenError {
    Name,
    Correlation,
    Completion,
    BufferTooSmall,
    Format(FormatError),
    InvalidProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerBookOpenUpdate {
    Request(RequestEnvelope),
    Complete,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Size,
    Prefix {
        file_len: u64,
        len: u16,
    },
    Table {
        header: BookIndexHeader,
        range: ReadRange,
        offset: usize,
        len: u16,
    },
    Profile {
        index: BookIndex,
        len: u16,
    },
    Rendition {
        index: BookIndex,
        len: u16,
    },
    Complete,
}

pub struct OwnerBookOpenClient<'a> {
    name: StorageName,
    target: &'a mut Option<OwnerBook>,
    phase: Phase,
    expected_sequence: u32,
    next_sequence: u32,
}

impl<'a> OwnerBookOpenClient<'a> {
    pub fn begin(
        name: &str,
        target: &'a mut Option<OwnerBook>,
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), OwnerBookOpenError> {
        let name = StorageName::new(name).map_err(|_| OwnerBookOpenError::Name)?;
        *target = None;
        let request = envelope(sequence, Request::FileSize { name: name.clone() });
        Ok((
            Self {
                name,
                target,
                phase: Phase::Size,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        ))
    }

    pub fn begin_cached(
        name: &str,
        mount_token: crate::library::MountFileToken,
        target: &'a mut Option<OwnerBook>,
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), OwnerBookOpenError> {
        let name = StorageName::new(name).map_err(|_| OwnerBookOpenError::Name)?;
        *target = None;
        let request = envelope(
            sequence,
            Request::ActivateFile {
                name: name.clone(),
                token: mount_token,
            },
        );
        Ok((
            Self {
                name,
                target,
                phase: Phase::Size,
                expected_sequence: sequence,
                next_sequence: sequence.wrapping_add(1),
            },
            request,
        ))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: &mut [u8],
    ) -> Result<OwnerBookOpenUpdate, OwnerBookOpenError> {
        if response.client != StorageClient::Reader || response.sequence != self.expected_sequence {
            return Err(OwnerBookOpenError::Correlation);
        }
        let response = response
            .response
            .map_err(|_| OwnerBookOpenError::Completion)?;
        match (self.phase, response) {
            (Phase::Size, Response::FileSize(file_len)) => {
                if buffer.len() < HEADER_BYTES {
                    return Err(OwnerBookOpenError::BufferTooSmall);
                }
                let len = file_len
                    .min(buffer.len() as u64)
                    .min(u64::from(MAX_BOUNDED_IO_BYTES)) as u16;
                Ok(self.issue_read(
                    ReadRange::from_parts(0, usize::from(len)),
                    0,
                    len,
                    Phase::Prefix { file_len, len },
                ))
            }
            (Phase::Prefix { file_len, len }, Response::Read(actual)) if actual == len => {
                let prefix = &buffer[..usize::from(actual)];
                let header = BookIndex::parse_header(prefix, file_len)
                    .map_err(OwnerBookOpenError::Format)?;
                let range = header.section_table_range();
                if range.length() > buffer.len() {
                    return Err(OwnerBookOpenError::BufferTooSmall);
                }
                let Some(table) = prefix_slice(prefix, range) else {
                    return Ok(self.issue_table_read(header, range, 0));
                };
                let index = BookIndex::parse(header, table).map_err(OwnerBookOpenError::Format)?;
                let profile_range = index.display_profile_range();
                let rendition_range = index.rendition_identity_range();
                let (Some(profile_bytes), Some(rendition_bytes)) = (
                    prefix_slice(prefix, profile_range),
                    prefix_slice(prefix, rendition_range),
                ) else {
                    let len = u16::try_from(profile_range.length())
                        .map_err(|_| OwnerBookOpenError::BufferTooSmall)?;
                    return Ok(self.issue_read(
                        profile_range,
                        0,
                        len,
                        Phase::Profile { index, len },
                    ));
                };
                let profile = index
                    .parse_display_profile(profile_bytes)
                    .map_err(OwnerBookOpenError::Format)?;
                xteink_x4_display::profile::validate_profile(&profile)
                    .map_err(|_| OwnerBookOpenError::InvalidProfile)?;
                let rendition_identity = index
                    .parse_rendition_identity(rendition_bytes)
                    .map_err(OwnerBookOpenError::Format)?;
                *self.target = Some(OwnerBook {
                    name: self.name.clone(),
                    index,
                    rendition_identity,
                });
                self.phase = Phase::Complete;
                Ok(OwnerBookOpenUpdate::Complete)
            }
            (
                Phase::Table {
                    header,
                    range,
                    offset,
                    len,
                },
                Response::Read(actual),
            ) if actual == len => {
                let next = offset + usize::from(len);
                if next < range.length() {
                    Ok(self.issue_table_read(header, range, next))
                } else {
                    let index = BookIndex::parse(header, &buffer[..range.length()])
                        .map_err(OwnerBookOpenError::Format)?;
                    let profile = index.display_profile_range();
                    let len = u16::try_from(profile.length())
                        .map_err(|_| OwnerBookOpenError::BufferTooSmall)?;
                    Ok(self.issue_read(profile, 0, len, Phase::Profile { index, len }))
                }
            }
            (Phase::Profile { index, len }, Response::Read(actual)) if actual == len => {
                let profile = index
                    .parse_display_profile(&buffer[..usize::from(len)])
                    .map_err(OwnerBookOpenError::Format)?;
                xteink_x4_display::profile::validate_profile(&profile)
                    .map_err(|_| OwnerBookOpenError::InvalidProfile)?;
                let rendition = index.rendition_identity_range();
                let len = u16::try_from(rendition.length())
                    .map_err(|_| OwnerBookOpenError::BufferTooSmall)?;
                Ok(self.issue_read(rendition, 0, len, Phase::Rendition { index, len }))
            }
            (Phase::Rendition { index, len }, Response::Read(actual)) if actual == len => {
                let rendition_identity = index
                    .parse_rendition_identity(&buffer[..usize::from(len)])
                    .map_err(OwnerBookOpenError::Format)?;
                *self.target = Some(OwnerBook {
                    name: self.name.clone(),
                    index,
                    rendition_identity,
                });
                self.phase = Phase::Complete;
                Ok(OwnerBookOpenUpdate::Complete)
            }
            _ => Err(OwnerBookOpenError::Completion),
        }
    }

    fn issue_table_read(
        &mut self,
        header: BookIndexHeader,
        range: ReadRange,
        offset: usize,
    ) -> OwnerBookOpenUpdate {
        let len = (range.length() - offset).min(MAX_BOUNDED_IO_BYTES as usize) as u16;
        self.issue_read(
            ReadRange::from_parts(range.offset() + offset as u64, usize::from(len)),
            offset as u16,
            len,
            Phase::Table {
                header,
                range,
                offset,
                len,
            },
        )
    }

    fn issue_read(
        &mut self,
        range: ReadRange,
        buffer_offset: u16,
        len: u16,
        phase: Phase,
    ) -> OwnerBookOpenUpdate {
        let sequence = self.take_sequence();
        self.phase = phase;
        OwnerBookOpenUpdate::Request(envelope(
            sequence,
            Request::read_into(
                self.name.clone(),
                range.offset(),
                buffer_offset,
                len,
                BufferSlot::Reader,
            )
            .expect("book-open reads are bounded"),
        ))
    }

    fn take_sequence(&mut self) -> u32 {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        self.expected_sequence = sequence;
        sequence
    }
}

fn envelope(sequence: u32, request: Request) -> RequestEnvelope {
    RequestEnvelope {
        client: StorageClient::Reader,
        sequence,
        request,
    }
}

fn prefix_slice(prefix: &[u8], range: ReadRange) -> Option<&[u8]> {
    let start = usize::try_from(range.offset()).ok()?;
    let end = start.checked_add(range.length())?;
    prefix.get(start..end)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerPageLoadError {
    Range(FormatError),
    Correlation,
    Completion,
    InvalidPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerPageLoadUpdate {
    Complete,
}

pub struct OwnerPageLoadClient<'a> {
    book: &'a OwnerBook,
    page: u32,
    target: &'a mut Option<PageInfo>,
    expected_sequence: u32,
    expected_len: u16,
    complete: bool,
}

impl<'a> OwnerPageLoadClient<'a> {
    pub fn begin(
        book: &'a OwnerBook,
        page: u32,
        target: &'a mut Option<PageInfo>,
        sequence: u32,
    ) -> Result<(Self, RequestEnvelope), OwnerPageLoadError> {
        *target = None;
        let range = book
            .index
            .page_record_range(page)
            .map_err(OwnerPageLoadError::Range)?;
        let len = u16::try_from(range.length()).map_err(|_| OwnerPageLoadError::InvalidPage)?;
        let request = envelope(
            sequence,
            Request::read(book.name.clone(), range.offset(), len, BufferSlot::Reader)
                .expect("page records are bounded"),
        );
        Ok((
            Self {
                book,
                page,
                target,
                expected_sequence: sequence,
                expected_len: len,
                complete: false,
            },
            request,
        ))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        buffer: &[u8],
    ) -> Result<OwnerPageLoadUpdate, OwnerPageLoadError> {
        if self.complete
            || response.client != StorageClient::Reader
            || response.sequence != self.expected_sequence
        {
            return Err(OwnerPageLoadError::Correlation);
        }
        match response.response {
            Ok(Response::Read(actual)) if actual == self.expected_len => {
                let page = self
                    .book
                    .index
                    .parse_page(self.page, &buffer[..usize::from(actual)])
                    .map_err(OwnerPageLoadError::Range)?;
                xteink_x4_display::profile::validate_page(&page)
                    .map_err(|_| OwnerPageLoadError::InvalidPage)?;
                *self.target = Some(page);
                self.complete = true;
                Ok(OwnerPageLoadUpdate::Complete)
            }
            _ => Err(OwnerPageLoadError::Completion),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerPlaneStreamError {
    Range(FormatError),
    Correlation,
    Completion,
    Decode,
    Buffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerPlaneStreamUpdate {
    Request(RequestEnvelope),
    RowsReady { row_count: u16, complete: bool },
}

pub struct OwnerPlaneStream<'a> {
    book: &'a OwnerBook,
    plane: binbook_core::PlaneDescriptor,
    decoder: Option<StreamingPlaneDecoder>,
    input_capacity: usize,
    expected_sequence: u32,
    next_sequence: u32,
    expected_read: u16,
    awaiting_read: bool,
    needs_read: bool,
    rows_decoded: u16,
    pending_row: [u8; xteink_x4_display::profile::ROW_BYTES],
}

impl<'a> OwnerPlaneStream<'a> {
    pub fn begin(
        book: &'a OwnerBook,
        plane: binbook_core::PlaneDescriptor,
        sequence: u32,
        input_capacity: usize,
    ) -> Result<(Self, RequestEnvelope), OwnerPlaneStreamError> {
        if input_capacity == 0 || input_capacity > MAX_BOUNDED_IO_BYTES as usize {
            return Err(OwnerPlaneStreamError::Buffer);
        }
        let mut stream = Self {
            book,
            plane,
            decoder: Some(StreamingPlaneDecoder::new(plane)),
            input_capacity,
            expected_sequence: sequence,
            next_sequence: sequence.wrapping_add(1),
            expected_read: 0,
            awaiting_read: false,
            needs_read: true,
            rows_decoded: 0,
            pending_row: [0; xteink_x4_display::profile::ROW_BYTES],
        };
        let request = stream.issue_read_with_sequence(sequence)?;
        Ok((stream, request))
    }

    pub fn accept(
        &mut self,
        response: ResponseEnvelope,
        input: &[u8],
        rows: &mut [u8],
    ) -> Result<OwnerPlaneStreamUpdate, OwnerPlaneStreamError> {
        if !self.awaiting_read
            || response.client != StorageClient::Reader
            || response.sequence != self.expected_sequence
        {
            return Err(OwnerPlaneStreamError::Correlation);
        }
        match response.response {
            Ok(Response::Read(actual)) if actual == self.expected_read => {
                if input.len() < usize::from(actual) {
                    return Err(OwnerPlaneStreamError::Buffer);
                }
                self.decoder
                    .as_mut()
                    .ok_or(OwnerPlaneStreamError::Decode)?
                    .accept_read(usize::from(actual))
                    .map_err(|_| OwnerPlaneStreamError::Decode)?;
                self.awaiting_read = false;
                self.needs_read = false;
                self.advance(input, rows)
            }
            _ => Err(OwnerPlaneStreamError::Completion),
        }
    }

    pub fn advance(
        &mut self,
        input: &[u8],
        rows: &mut [u8],
    ) -> Result<OwnerPlaneStreamUpdate, OwnerPlaneStreamError> {
        const ROW_BYTES: usize = xteink_x4_display::profile::ROW_BYTES;
        if rows.len() < ROW_BYTES || rows.len() % ROW_BYTES != 0 {
            return Err(OwnerPlaneStreamError::Buffer);
        }
        if self.needs_read {
            let sequence = self.next_sequence;
            self.next_sequence = sequence.wrapping_add(1);
            return self
                .issue_read_with_sequence(sequence)
                .map(OwnerPlaneStreamUpdate::Request);
        }
        let capacity = rows.len() / ROW_BYTES;
        let mut ready = 0usize;
        while ready < capacity {
            match self
                .decoder
                .as_mut()
                .ok_or(OwnerPlaneStreamError::Decode)?
                .decode_row(input, &mut self.pending_row)
                .map_err(|_| OwnerPlaneStreamError::Decode)?
            {
                PlaneDecodeStep::RowComplete => {
                    let start = ready * ROW_BYTES;
                    rows[start..start + ROW_BYTES].copy_from_slice(&self.pending_row);
                    ready += 1;
                    self.rows_decoded += 1;
                    if self.rows_decoded == xteink_x4_display::profile::PHYSICAL_HEIGHT {
                        self.decoder
                            .take()
                            .ok_or(OwnerPlaneStreamError::Decode)?
                            .finish()
                            .map_err(|_| OwnerPlaneStreamError::Decode)?;
                        return Ok(OwnerPlaneStreamUpdate::RowsReady {
                            row_count: ready as u16,
                            complete: true,
                        });
                    }
                }
                PlaneDecodeStep::NeedInput => {
                    self.needs_read = true;
                    if ready != 0 {
                        return Ok(OwnerPlaneStreamUpdate::RowsReady {
                            row_count: ready as u16,
                            complete: false,
                        });
                    }
                    let sequence = self.next_sequence;
                    self.next_sequence = sequence.wrapping_add(1);
                    return self
                        .issue_read_with_sequence(sequence)
                        .map(OwnerPlaneStreamUpdate::Request);
                }
            }
        }
        Ok(OwnerPlaneStreamUpdate::RowsReady {
            row_count: ready as u16,
            complete: false,
        })
    }

    fn issue_read_with_sequence(
        &mut self,
        sequence: u32,
    ) -> Result<RequestEnvelope, OwnerPlaneStreamError> {
        let request = self
            .decoder
            .as_mut()
            .ok_or(OwnerPlaneStreamError::Decode)?
            .next_read(self.input_capacity)
            .map_err(|_| OwnerPlaneStreamError::Decode)?;
        let relative = self
            .plane
            .offset
            .get()
            .checked_add(u64::from(request.offset))
            .ok_or(OwnerPlaneStreamError::Range(FormatError::InvalidPage))?;
        let range = self
            .book
            .index
            .page_data_range(relative, request.len as u32)
            .map_err(OwnerPlaneStreamError::Range)?;
        let len = request.len as u16;
        self.expected_sequence = sequence;
        self.expected_read = len;
        self.awaiting_read = true;
        self.needs_read = false;
        Ok(envelope(
            sequence,
            Request::read(
                self.book.name.clone(),
                range.offset(),
                len,
                BufferSlot::Reader,
            )
            .expect("plane reads are bounded"),
        ))
    }
}
