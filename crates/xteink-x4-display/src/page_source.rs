use binbook_core::{Book, PageInfo, PlaneDescriptor, ReadAt, PAGE_RECORD_SIZE};
use binbook_decompress::PackBitsDecoder;

use crate::{buffers::RenderBuffers, profile, stream::decode_stream, DisplayError, DisplayResult};

pub fn read_x4_page<R: ReadAt>(book: &mut Book<R>, raw: u32) -> DisplayResult<PageInfo> {
    let mut profile_record = [0_u8; 56];
    let display_profile = book
        .display_profile(&mut profile_record)
        .map_err(map_core_error)?;
    profile::validate_profile(&display_profile)?;
    let number = book.page_number(raw).map_err(|_| DisplayError::Format)?;
    let mut record = [0_u8; PAGE_RECORD_SIZE];
    let page = book.page(number, &mut record).map_err(map_core_error)?;
    profile::validate_page(&page)?;
    Ok(page)
}

fn map_core_error<E>(error: binbook_core::Error<E>) -> DisplayError {
    match error {
        binbook_core::Error::Source(_) => DisplayError::Source,
        binbook_core::Error::Format(_) => DisplayError::Format,
        binbook_core::Error::BufferTooSmall { required, provided } => {
            DisplayError::BufferTooSmall { required, provided }
        }
    }
}

pub(crate) fn required_plane(
    planes: binbook_core::PlaneDirectory,
    slot: binbook_core::PlaneSlot,
) -> DisplayResult<PlaneDescriptor> {
    planes.get(slot).ok_or(DisplayError::InvalidPage)
}

pub fn decode_plane<R, F>(
    book: &mut Book<R>,
    plane: PlaneDescriptor,
    decoded_len: usize,
    buffers: &mut RenderBuffers<'_>,
    emit: F,
) -> DisplayResult<()>
where
    R: ReadAt,
    F: FnMut(&[u8]) -> DisplayResult<()>,
{
    let compressed_len = usize::try_from(plane.length.get()).map_err(|_| DisplayError::Format)?;
    let method = plane.compression;
    let mut source = PlaneSource { book, plane };
    decode_stream(
        &mut source,
        0,
        compressed_len,
        method,
        decoded_len,
        buffers,
        emit,
    )
}

struct PlaneSource<'a, R: ReadAt> {
    book: &'a mut Book<R>,
    plane: PlaneDescriptor,
}

pub struct PlaneDecoder {
    plane: PlaneDescriptor,
    source_position: u32,
    input_position: usize,
    input_length: usize,
    decoder: PackBitsDecoder,
}

impl PlaneDecoder {
    #[must_use]
    pub const fn new(plane: PlaneDescriptor) -> Self {
        Self {
            plane,
            source_position: 0,
            input_position: 0,
            input_length: 0,
            decoder: PackBitsDecoder::new(),
        }
    }

    pub fn fill<R: ReadAt>(
        &mut self,
        book: &mut Book<R>,
        input: &mut [u8],
        output: &mut [u8],
    ) -> DisplayResult<()> {
        if input.is_empty() {
            return Err(DisplayError::BufferTooSmall {
                required: 1,
                provided: 0,
            });
        }
        match self.plane.compression {
            binbook_core::CompressionMethod::None => {
                book.read_plane_range(self.plane, self.source_position, output)
                    .map_err(|_| DisplayError::Source)?;
                self.source_position = self
                    .source_position
                    .checked_add(u32::try_from(output.len()).map_err(|_| DisplayError::Format)?)
                    .ok_or(DisplayError::Format)?;
                Ok(())
            }
            binbook_core::CompressionMethod::RlePackBits => self.fill_packbits(book, input, output),
            binbook_core::CompressionMethod::Lz4 => Err(DisplayError::Decode),
        }
    }

    fn fill_packbits<R: ReadAt>(
        &mut self,
        book: &mut Book<R>,
        input: &mut [u8],
        output: &mut [u8],
    ) -> DisplayResult<()> {
        let mut written = 0;
        while written < output.len() {
            let progress = self.decoder.decode(
                &input[self.input_position..self.input_length],
                &mut output[written..],
            )?;
            self.input_position += progress.consumed;
            written += progress.produced;
            if progress.consumed == 0 && progress.produced == 0 {
                let remaining = self
                    .plane
                    .length
                    .get()
                    .checked_sub(self.source_position)
                    .ok_or(DisplayError::Decode)?;
                if self.input_position != self.input_length || remaining == 0 {
                    return Err(DisplayError::Decode);
                }
                self.input_length = input
                    .len()
                    .min(usize::try_from(remaining).unwrap_or(usize::MAX));
                book.read_plane_range(
                    self.plane,
                    self.source_position,
                    &mut input[..self.input_length],
                )
                .map_err(|_| DisplayError::Source)?;
                self.source_position = self
                    .source_position
                    .checked_add(
                        u32::try_from(self.input_length).map_err(|_| DisplayError::Format)?,
                    )
                    .ok_or(DisplayError::Format)?;
                self.input_position = 0;
            }
        }
        Ok(())
    }

    pub fn finish(self) -> DisplayResult<()> {
        if self.source_position != self.plane.length.get()
            || self.input_position != self.input_length
            || !self.decoder.is_idle()
        {
            return Err(DisplayError::Decode);
        }
        self.decoder.finish()?;
        Ok(())
    }
}

impl<R: ReadAt> ReadAt for PlaneSource<'_, R> {
    type Error = binbook_core::Error<R::Error>;

    fn len(&mut self) -> Result<u64, Self::Error> {
        Ok(u64::from(self.plane.length.get()))
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), Self::Error> {
        let offset = u32::try_from(offset)
            .map_err(|_| binbook_core::Error::Format(binbook_core::FormatError::InvalidPage))?;
        self.book.read_plane_range(self.plane, offset, out)
    }
}
