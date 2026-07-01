use binbook_core::CompressionMethod;
use binbook_encode::{CompiledChunk, CompiledPage, CompiledPlane};
use gray2_render::{
    canonical_image_to_staged, quantize_gray1, quantize_gray2, FloydSteinberg, PlaneChunks,
};
use xteink_x4_display::profile::{
    logical_gray2_to_physical_packed, CHUNK_ROWS, LOGICAL_HEIGHT, LOGICAL_WIDTH, PHYSICAL_HEIGHT,
    PHYSICAL_WIDTH, ROW_BYTES,
};

use crate::{decode_image, fit_luma, ImageError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFormat {
    Gray1,
    Gray2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileOptions {
    pub storage_format: StorageFormat,
    pub dither: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            storage_format: StorageFormat::Gray2,
            dither: true,
        }
    }
}

pub fn compile_image(bytes: &[u8], options: CompileOptions) -> Result<CompiledPage, ImageError> {
    let decoded = decode_image(bytes)?;
    let logical = fit_luma(
        &decoded,
        u32::from(LOGICAL_WIDTH),
        u32::from(LOGICAL_HEIGHT),
    )?;
    let levels = quantize(&logical.pixels, options)?;
    let mut physical = vec![0_u8; usize::from(PHYSICAL_WIDTH) * usize::from(PHYSICAL_HEIGHT) / 4];
    logical_gray2_to_physical_packed(&levels, &mut physical).map_err(|_| ImageError::Format)?;
    let plane_bytes = ROW_BYTES * usize::from(PHYSICAL_HEIGHT);
    let mut msb = vec![0_u8; plane_bytes];
    let mut lsb = vec![0_u8; plane_bytes];
    let mut base = vec![0_u8; plane_bytes];
    canonical_image_to_staged(
        &physical,
        usize::from(PHYSICAL_WIDTH),
        usize::from(PHYSICAL_HEIGHT),
        &mut msb,
        &mut lsb,
        &mut base,
    )?;
    let planes = match options.storage_format {
        StorageFormat::Gray1 => vec![compile_plane(2, &base)?],
        StorageFormat::Gray2 => vec![
            compile_plane(0, &msb)?,
            compile_plane(1, &lsb)?,
            compile_plane(2, &base)?,
        ],
    };
    Ok(CompiledPage {
        page_kind: 2,
        pixel_format: match options.storage_format {
            StorageFormat::Gray1 => 1,
            StorageFormat::Gray2 => 2,
        },
        stored_width: PHYSICAL_WIDTH,
        stored_height: PHYSICAL_HEIGHT,
        source_spine_index: u32::MAX,
        chapter_nav_index: u32::MAX,
        planes,
    })
}

fn quantize(luma: &[u8], options: CompileOptions) -> Result<Vec<u8>, ImageError> {
    let width = usize::from(LOGICAL_WIDTH);
    let height = usize::from(LOGICAL_HEIGHT);
    if luma.len() != width * height {
        return Err(ImageError::InvalidDimensions);
    }
    let mut output = vec![0_u8; luma.len()];
    if options.dither {
        let mut current = vec![0_f32; width + 2];
        let mut next = vec![0_f32; width + 2];
        let mut state = FloydSteinberg::new(width, &mut current, &mut next)?;
        for row in 0..height {
            let input = &luma[row * width..(row + 1) * width];
            let target = &mut output[row * width..(row + 1) * width];
            match options.storage_format {
                StorageFormat::Gray1 => state.quantize_gray1_row(input, target)?,
                StorageFormat::Gray2 => state.quantize_gray2_row(input, target)?,
            }
        }
    } else {
        for (target, source) in output.iter_mut().zip(luma.iter().copied()) {
            *target = match options.storage_format {
                StorageFormat::Gray1 => quantize_gray1(source),
                StorageFormat::Gray2 => quantize_gray2(source),
            };
        }
    }
    if options.storage_format == StorageFormat::Gray1 {
        for value in &mut output {
            *value *= 3;
        }
    }
    Ok(output)
}

fn compile_plane(slot: u8, plane: &[u8]) -> Result<CompiledPlane, ImageError> {
    let chunks = PlaneChunks::new(plane, ROW_BYTES, usize::from(CHUNK_ROWS))?
        .enumerate()
        .map(|(index, chunk)| {
            Ok(CompiledChunk {
                compressed: binbook_compress::encode(chunk),
                row_start: u16::try_from(index * usize::from(CHUNK_ROWS))
                    .map_err(|_| ImageError::InvalidDimensions)?,
                row_count: CHUNK_ROWS,
                uncompressed_size: u32::try_from(chunk.len())
                    .map_err(|_| ImageError::InvalidDimensions)?,
            })
        })
        .collect::<Result<Vec<_>, ImageError>>()?;
    Ok(CompiledPlane {
        slot,
        compression: CompressionMethod::RlePackBits,
        chunks,
    })
}
