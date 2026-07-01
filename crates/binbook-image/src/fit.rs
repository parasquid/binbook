use image::{imageops::FilterType, GrayImage, ImageBuffer, Luma, Rgba};

use crate::{DecodedImage, ImageError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LumaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn fit_luma(
    source: &DecodedImage,
    target_width: u32,
    target_height: u32,
) -> Result<LumaImage, ImageError> {
    if source.width == 0 || source.height == 0 || target_width == 0 || target_height == 0 {
        return Err(ImageError::InvalidDimensions);
    }
    let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
        source.width,
        source.height,
        source.rgba.clone(),
    )
    .ok_or(ImageError::Decode)?;
    let mut flattened = GrayImage::new(source.width, source.height);
    for (x, y, pixel) in image.enumerate_pixels() {
        flattened.put_pixel(x, y, Luma([composite_white(pixel.0)]));
    }
    let scale = (f64::from(target_width) / f64::from(source.width))
        .min(f64::from(target_height) / f64::from(source.height));
    let width = (f64::from(source.width) * scale).round().max(1.0) as u32;
    let height = (f64::from(source.height) * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(&flattened, width, height, FilterType::Lanczos3);
    let output_len = usize::try_from(u64::from(target_width) * u64::from(target_height))
        .map_err(|_| ImageError::InvalidDimensions)?;
    let mut pixels = vec![255_u8; output_len];
    let origin_x = (target_width - width) / 2;
    let origin_y = (target_height - height) / 2;
    for y in 0..height {
        for x in 0..width {
            let index = usize::try_from(
                u64::from(origin_y + y) * u64::from(target_width) + u64::from(origin_x + x),
            )
            .map_err(|_| ImageError::InvalidDimensions)?;
            pixels[index] = resized.get_pixel(x, y).0[0];
        }
    }
    Ok(LumaImage {
        width: target_width,
        height: target_height,
        pixels,
    })
}

fn composite_white(pixel: [u8; 4]) -> u8 {
    let alpha = u32::from(pixel[3]);
    let luma =
        (299 * u32::from(pixel[0]) + 587 * u32::from(pixel[1]) + 114 * u32::from(pixel[2]) + 500)
            / 1_000;
    ((luma * alpha + 255 * (255 - alpha) + 127) / 255) as u8
}
