use crate::header::{read_u16, read_u32};
use crate::{
    ByteLength, CompressionMethod, FileOffset, FormatError, PageNumber, PixelFormat, PlaneSlot,
};

pub const PAGE_RECORD_SIZE: usize = crate::index_encode::PAGE_INDEX_RECORD_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDescriptor {
    pub slot: PlaneSlot,
    pub compression: CompressionMethod,
    pub offset: FileOffset,
    pub length: ByteLength,
}

impl PlaneDescriptor {
    #[must_use]
    pub const fn new(
        slot: PlaneSlot,
        compression: CompressionMethod,
        offset: FileOffset,
        length: ByteLength,
    ) -> Self {
        Self {
            slot,
            compression,
            offset,
            length,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDirectory {
    bitmap: u8,
    slots: [Option<PlaneDescriptor>; 4],
}

impl PlaneDirectory {
    #[must_use]
    pub const fn new(slots: [Option<PlaneDescriptor>; 4]) -> Self {
        let mut bitmap = 0_u8;
        let mut index = 0_usize;
        while index < slots.len() {
            if slots[index].is_some() {
                bitmap |= 1 << index;
            }
            index += 1;
        }
        Self { bitmap, slots }
    }

    #[must_use]
    pub const fn bitmap(self) -> u8 {
        self.bitmap
    }

    #[must_use]
    pub const fn get(self, slot: PlaneSlot) -> Option<PlaneDescriptor> {
        self.slots[slot.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageInfo {
    pub page_number: PageNumber,
    pub page_kind: u16,
    pub pixel_format: PixelFormat,
    pub compression_method: CompressionMethod,
    pub update_hint: u16,
    pub page_flags: u32,
    pub page_crc32: u32,
    pub stored_width: u16,
    pub stored_height: u16,
    pub placement_x: u16,
    pub placement_y: u16,
    pub progress_start_ppm: u32,
    pub progress_end_ppm: u32,
    pub planes: PlaneDirectory,
}

pub(crate) fn parse(
    bytes: &[u8],
    expected: PageNumber,
    page_data_length: u64,
) -> Result<PageInfo, FormatError> {
    if bytes.len() < PAGE_RECORD_SIZE {
        return Err(FormatError::InvalidPage);
    }
    if read_u32(bytes, 0)? != expected.get() {
        return Err(FormatError::InvalidPage);
    }
    let default_compression = CompressionMethod::try_from(read_u16(bytes, 8)?)?;
    let page_flags = read_u32(bytes, 12)?;
    let bitmap = bytes[44];
    if bitmap & !0x0f != 0 {
        return Err(FormatError::InvalidPage);
    }
    let mut slots = [None; 4];
    for raw_slot in 0_u8..4 {
        if bitmap & (1 << raw_slot) == 0 {
            continue;
        }
        let slot = PlaneSlot::try_from(raw_slot)?;
        let index = usize::from(raw_slot);
        let offset = read_u32(bytes, 52 + index * 4)?;
        let length = read_u32(bytes, 68 + index * 4)?;
        let end = u64::from(offset)
            .checked_add(u64::from(length))
            .ok_or(FormatError::InvalidPage)?;
        if length == 0 || end > page_data_length {
            return Err(FormatError::InvalidPage);
        }
        let compression = if page_flags & 1 == 0 {
            default_compression
        } else {
            CompressionMethod::try_from(bytes[45 + index])?
        };
        slots[index] = Some(PlaneDescriptor {
            slot,
            compression,
            offset: FileOffset::from_validated(u64::from(offset)),
            length: ByteLength::from_validated(length),
        });
    }
    let progress_start_ppm = read_u32(bytes, 36)?;
    let progress_end_ppm = read_u32(bytes, 40)?;
    if progress_start_ppm > progress_end_ppm || progress_end_ppm > 1_000_000 {
        return Err(FormatError::InvalidPage);
    }
    Ok(PageInfo {
        page_number: expected,
        page_kind: read_u16(bytes, 4)?,
        pixel_format: PixelFormat::try_from(read_u16(bytes, 6)?)?,
        compression_method: default_compression,
        update_hint: read_u16(bytes, 10)?,
        page_flags,
        page_crc32: read_u32(bytes, 16)?,
        stored_width: read_u16(bytes, 20)?,
        stored_height: read_u16(bytes, 22)?,
        placement_x: read_u16(bytes, 24)?,
        placement_y: read_u16(bytes, 26)?,
        progress_start_ppm,
        progress_end_ppm,
        planes: PlaneDirectory { bitmap, slots },
    })
}
