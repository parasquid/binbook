use binbook_core::StringRef;

use crate::hashing::set_section_hash;
use crate::model::{BookConfig, CompiledPage, ModelError};
use crate::strings::StringTable;

pub(crate) fn build_display(
    config: &BookConfig,
    strings: &mut StringTable,
) -> Result<(Vec<u8>, [u8; 32]), ModelError> {
    let mut out = Vec::with_capacity(120);
    push_ref(&mut out, strings.add(&config.profile_id)?);
    push_ref(&mut out, strings.add(&config.device_family)?);
    push_ref(&mut out, strings.add(&config.device_model)?);
    for value in [
        config.logical_width,
        config.logical_height,
        config.physical_width,
        config.physical_height,
    ] {
        push_u16(&mut out, value);
    }
    out.push(1);
    out.extend_from_slice(&config.rotation_degrees.to_le_bytes());
    out.push(1);
    let format_flag = pixel_format_flag(config.storage_pixel_format);
    push_u32(&mut out, format_flag);
    push_u32(&mut out, format_flag);
    push_u16(&mut out, config.storage_pixel_format);
    push_u16(&mut out, 0);
    push_u16(&mut out, config.grayscale_levels);
    push_u16(&mut out, config.grayscale_levels);
    out.push(config.framebuffer_bits_per_pixel);
    push_u16(&mut out, config.waveform_hint);
    out.push(0);
    out.resize(120, 0);
    let hash = set_section_hash(&mut out, 56);
    Ok((out, hash))
}

pub(crate) fn build_layout(config: &BookConfig) -> (Vec<u8>, [u8; 32]) {
    let mut out = Vec::with_capacity(100);
    for value in [
        config.logical_width,
        config.logical_height,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        config.logical_width,
        config.logical_height,
    ] {
        push_u16(&mut out, value);
    }
    out.extend_from_slice(&[1, 1, 0, 0]);
    push_u16(&mut out, config.grayscale_levels.saturating_sub(1));
    push_u16(&mut out, 0);
    push_u32(&mut out, 0);
    out.resize(100, 0);
    let hash = set_section_hash(&mut out, 36);
    (out, hash)
}

pub(crate) fn build_requirements(
    config: &BookConfig,
    pages: &[CompiledPage],
) -> Result<Vec<u8>, ModelError> {
    let max_width = pages
        .iter()
        .map(|page| page.stored_width)
        .max()
        .unwrap_or(0);
    let max_height = pages
        .iter()
        .map(|page| page.stored_height)
        .max()
        .unwrap_or(0);
    let mut max_uncompressed = 0_u32;
    let mut max_compressed = 0_u32;
    for plane in pages.iter().flat_map(|page| &page.planes) {
        let uncompressed = plane.chunks.iter().try_fold(0_u32, |total, chunk| {
            total
                .checked_add(chunk.uncompressed_size)
                .ok_or(ModelError::LengthOverflow)
        })?;
        let compressed = plane.chunks.iter().try_fold(0_u32, |total, chunk| {
            let length =
                u32::try_from(chunk.compressed.len()).map_err(|_| ModelError::LengthOverflow)?;
            total.checked_add(length).ok_or(ModelError::LengthOverflow)
        })?;
        max_uncompressed = max_uncompressed.max(uncompressed);
        max_compressed = max_compressed.max(compressed);
    }
    let mut out = Vec::with_capacity(76);
    push_u64(&mut out, (1 << 0) | (1 << 3));
    push_u64(&mut out, (1 << 0) | (1 << 2) | (1 << 3) | (1 << 4));
    push_u32(&mut out, pixel_format_flag(config.storage_pixel_format));
    push_u16(&mut out, config.grayscale_levels);
    push_u16(&mut out, 1);
    push_u32(&mut out, 1 << 1);
    push_u16(&mut out, max_width);
    push_u16(&mut out, max_height);
    push_u32(&mut out, max_uncompressed);
    push_u32(&mut out, max_compressed);
    out.resize(76, 0);
    Ok(out)
}

fn pixel_format_flag(format: u16) -> u32 {
    match format {
        1 => 1,
        2 => 2,
        4 => 4,
        8 => 8,
        16 => 16,
        32 => 32,
        _ => 0,
    }
}

fn push_ref(out: &mut Vec<u8>, value: StringRef) {
    push_u32(out, value.offset);
    push_u32(out, value.length);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
