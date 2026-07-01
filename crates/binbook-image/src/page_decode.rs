use std::io::Cursor;

use binbook_core::{Book, CompressionMethod, PixelFormat, PlaneDescriptor, PlaneSlot, SliceSource};
use image::{DynamicImage, GrayImage, ImageFormat, Luma};

use crate::ImageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPage {
    pub width: u16,
    pub height: u16,
    pub pixel_format: PixelFormat,
    pub packed: Vec<u8>,
}

pub fn decode_blob(
    method: CompressionMethod,
    input: &[u8],
    output: &mut [u8],
) -> Result<(), ImageError> {
    binbook_decompress::decode_exact(method, input, output)?;
    Ok(())
}

pub fn decode_book_page(bytes: &[u8], page_index: u32) -> Result<DecodedPage, ImageError> {
    let mut scratch = [0_u8; 1024];
    let mut book =
        Book::open(SliceSource::new(bytes), &mut scratch).map_err(|_| ImageError::Format)?;
    let number = book
        .page_number(page_index)
        .map_err(|_| ImageError::InvalidPage)?;
    let page = book
        .page(number, &mut scratch)
        .map_err(|_| ImageError::Format)?;
    let row_bytes = usize::from(page.stored_width) / 8;
    let plane_length = row_bytes * usize::from(page.stored_height);
    let base = decode_plane(
        &mut book,
        page.planes.get(PlaneSlot::FastBase),
        plane_length,
    )?;
    let packed = match page.pixel_format {
        PixelFormat::Gray1Packed => native_base_to_gray1(&base, row_bytes, page.stored_height),
        PixelFormat::Gray2Packed => {
            let msb = decode_plane(
                &mut book,
                page.planes.get(PlaneSlot::OverlayMsb),
                plane_length,
            )?;
            let lsb = decode_plane(
                &mut book,
                page.planes.get(PlaneSlot::OverlayLsb),
                plane_length,
            )?;
            native_staged_to_gray2(&msb, &lsb, &base, page.stored_width, page.stored_height)
        }
        _ => return Err(ImageError::UnsupportedFormat),
    };
    Ok(DecodedPage {
        width: page.stored_width,
        height: page.stored_height,
        pixel_format: page.pixel_format,
        packed,
    })
}

pub fn encode_page_png(page: &DecodedPage) -> Result<Vec<u8>, ImageError> {
    let mut image = GrayImage::new(u32::from(page.width), u32::from(page.height));
    for y in 0..usize::from(page.height) {
        for x in 0..usize::from(page.width) {
            let value = match page.pixel_format {
                PixelFormat::Gray1Packed => {
                    let row = usize::from(page.width).div_ceil(8);
                    let bit = (page.packed[y * row + x / 8] >> (7 - x % 8)) & 1;
                    if bit == 0 {
                        0
                    } else {
                        255
                    }
                }
                PixelFormat::Gray2Packed => {
                    let row = usize::from(page.width).div_ceil(4);
                    let level = (page.packed[y * row + x / 4] >> (6 - (x % 4) * 2)) & 3;
                    level * 85
                }
                _ => return Err(ImageError::UnsupportedFormat),
            };
            image.put_pixel(x as u32, y as u32, Luma([value]));
        }
    }
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| ImageError::Encode)?;
    Ok(output.into_inner())
}

fn decode_plane(
    book: &mut Book<SliceSource<'_>>,
    plane: Option<PlaneDescriptor>,
    output_length: usize,
) -> Result<Vec<u8>, ImageError> {
    let plane = plane.ok_or(ImageError::InvalidPage)?;
    let mut compressed =
        vec![0_u8; usize::try_from(plane.length.get()).map_err(|_| ImageError::InvalidDimensions)?];
    book.read_plane(plane, &mut compressed)
        .map_err(|_| ImageError::Format)?;
    let mut output = vec![0_u8; output_length];
    decode_blob(plane.compression, &compressed, &mut output)?;
    Ok(output)
}

fn native_base_to_gray1(base: &[u8], row_bytes: usize, height: u16) -> Vec<u8> {
    let mut output = vec![0_u8; base.len()];
    for y in 0..usize::from(height) {
        for x in 0..row_bytes * 8 {
            let ram_x = row_bytes * 8 - 1 - x;
            let bit = (base[y * row_bytes + ram_x / 8] >> (7 - ram_x % 8)) & 1;
            output[y * row_bytes + x / 8] |= bit << (7 - x % 8);
        }
    }
    output
}

fn native_staged_to_gray2(msb: &[u8], lsb: &[u8], base: &[u8], width: u16, height: u16) -> Vec<u8> {
    let row_bytes = usize::from(width) / 8;
    let packed_row = usize::from(width) / 4;
    let mut output = vec![0_u8; packed_row * usize::from(height)];
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            let ram_x = usize::from(width) - 1 - x;
            let mask = 0x80 >> (ram_x % 8);
            let index = y * row_bytes + ram_x / 8;
            let level = if base[index] & mask != 0 {
                3
            } else if msb[index] & mask == 0 {
                0
            } else if lsb[index] & mask != 0 {
                1
            } else {
                2
            };
            output[y * packed_row + x / 4] |= level << (6 - (x % 4) * 2);
        }
    }
    output
}
