use std::io::{BufReader, Cursor};

use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{DynamicImage, ImageFormat as ExternalFormat};

use crate::ImageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
    Svg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    if looks_like_svg(bytes) {
        return decode_svg(bytes);
    }
    let format = image::guess_format(bytes).map_err(|_| ImageError::UnsupportedFormat)?;
    let (image, format) = match format {
        ExternalFormat::Png => (decode_png(bytes)?, ImageFormat::Png),
        ExternalFormat::Jpeg => (
            image::load_from_memory_with_format(bytes, ExternalFormat::Jpeg)
                .map_err(|_| ImageError::Decode)?,
            ImageFormat::Jpeg,
        ),
        ExternalFormat::WebP => (decode_webp(bytes)?, ImageFormat::WebP),
        _ => return Err(ImageError::UnsupportedFormat),
    };
    from_dynamic(image, format)
}

fn decode_png(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    let decoder = PngDecoder::new(Cursor::new(bytes)).map_err(|_| ImageError::Decode)?;
    if decoder.is_apng().map_err(|_| ImageError::Decode)? {
        return Err(ImageError::AnimatedImage);
    }
    DynamicImage::from_decoder(decoder).map_err(|_| ImageError::Decode)
}

fn decode_webp(bytes: &[u8]) -> Result<DynamicImage, ImageError> {
    let decoder =
        WebPDecoder::new(BufReader::new(Cursor::new(bytes))).map_err(|_| ImageError::Decode)?;
    if decoder.has_animation() {
        return Err(ImageError::AnimatedImage);
    }
    DynamicImage::from_decoder(decoder).map_err(|_| ImageError::Decode)
}

fn decode_svg(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|_| ImageError::Decode)?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or(ImageError::InvalidDimensions)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(DecodedImage {
        format: ImageFormat::Svg,
        width: size.width(),
        height: size.height(),
        rgba: pixmap.take(),
    })
}

fn from_dynamic(image: DynamicImage, format: ImageFormat) -> Result<DecodedImage, ImageError> {
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(ImageError::InvalidDimensions);
    }
    Ok(DecodedImage {
        format,
        width,
        height,
        rgba: image.into_raw(),
    })
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .is_some_and(|text| text.trim_start().starts_with("<svg") || text.contains("<svg "))
}
