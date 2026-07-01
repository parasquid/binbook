use std::io::Cursor;

use binbook_image::{decode_image, fit_luma, ImageError, ImageFormat};
use image::{DynamicImage, ImageBuffer, ImageFormat as ExternalFormat, Rgba};

#[test]
fn decodes_png_jpeg_webp_and_svg_from_memory() {
    let source = DynamicImage::ImageRgba8(ImageBuffer::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    }));
    for (external, expected) in [
        (ExternalFormat::Png, ImageFormat::Png),
        (ExternalFormat::Jpeg, ImageFormat::Jpeg),
        (ExternalFormat::WebP, ImageFormat::WebP),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, external).unwrap();
        let decoded = decode_image(&bytes.into_inner()).unwrap();
        assert_eq!(decoded.format, expected);
        assert_eq!((decoded.width, decoded.height), (2, 1));
    }

    let decoded = decode_image(include_bytes!("fixtures/two-color.svg")).unwrap();
    assert_eq!(decoded.format, ImageFormat::Svg);
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(&decoded.rgba[..4], &[0, 0, 0, 255]);
    assert_eq!(&decoded.rgba[4..8], &[255, 255, 255, 255]);
}

#[test]
fn alpha_is_composited_over_white_and_contain_is_centered() {
    let image = binbook_image::DecodedImage {
        format: ImageFormat::Png,
        width: 2,
        height: 1,
        rgba: vec![0, 0, 0, 0, 0, 0, 0, 255],
    };
    let fitted = fit_luma(&image, 4, 4).unwrap();
    assert_eq!((fitted.width, fitted.height), (4, 4));
    assert!(fitted.pixels[..4].iter().all(|value| *value == 255));
    assert!(fitted.pixels[12..].iter().all(|value| *value == 255));
    assert!(fitted.pixels[4] >= 250);
    assert!(fitted.pixels[7] < 10);
}

#[test]
fn rejects_malformed_and_animated_png() {
    assert!(matches!(
        decode_image(b"not an image"),
        Err(ImageError::UnsupportedFormat)
    ));

    let mut animated = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut animated, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0]).unwrap();
        writer.write_image_data(&[255]).unwrap();
    }
    assert!(matches!(
        decode_image(&animated),
        Err(ImageError::AnimatedImage)
    ));
}

#[test]
fn resized_luma_stays_within_python_lanczos_parity_bound() {
    let image = binbook_image::DecodedImage {
        format: ImageFormat::Png,
        width: 3,
        height: 2,
        rgba: vec![
            0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 128,
            0, 0, 255, 0,
        ],
    };
    let expected = [
        0, 0, 32, 113, 198, 246, 255, 5, 15, 56, 134, 209, 248, 255, 30, 48, 92, 165, 226, 251,
        255, 55, 80, 127, 196, 242, 253, 255, 72, 103, 151, 217, 253, 255, 255,
    ];
    let actual = fit_luma(&image, 7, 5).unwrap();
    let squared_error: f64 = actual
        .pixels
        .iter()
        .zip(expected)
        .map(|(actual, expected)| {
            let difference = f64::from(*actual) - f64::from(expected);
            difference * difference
        })
        .sum();
    let rmse = (squared_error / expected.len() as f64).sqrt();
    assert!(rmse <= 3.0, "RMSE {rmse}");
}

#[test]
fn portrait_contain_uses_centered_horizontal_padding() {
    let image = binbook_image::DecodedImage {
        format: ImageFormat::Png,
        width: 1,
        height: 2,
        rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
    };
    let fitted = fit_luma(&image, 4, 4).unwrap();
    for row in fitted.pixels.chunks_exact(4) {
        assert_eq!(row[0], 255);
        assert_eq!(row[3], 255);
    }
    assert!(fitted.pixels[1] < fitted.pixels[13]);
}
