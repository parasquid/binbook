use std::io::Cursor;

use binbook_core::{Book, CompressionMethod, PlaneSlot, SliceSource};
use binbook_encode::{BookBuilder, BookConfig};
use binbook_image::{
    compile_image, decode_blob, decode_book_page, encode_page_png, CompileOptions, StorageFormat,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
use xteink_x4_display::profile::{logical_to_physical, LOGICAL_HEIGHT, LOGICAL_WIDTH};

fn gradient_png(width: u32, height: u32) -> Vec<u8> {
    let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(width, height, |x, y| {
        Luma([((x + y) % 256) as u8])
    }));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn compiles_staged_gray2_and_fast_gray1_with_exact_x4_chunks() {
    let source = gradient_png(32, 48);
    let gray2 = compile_image(
        &source,
        CompileOptions {
            storage_format: StorageFormat::Gray2,
            dither: true,
        },
    )
    .unwrap();
    assert_eq!(
        (gray2.pixel_format, gray2.stored_width, gray2.stored_height),
        (2, 800, 480)
    );
    assert_eq!(gray2.planes.len(), 3);
    assert!(gray2.planes.iter().all(|plane| plane.chunks.len() == 30));
    assert!(gray2
        .planes
        .iter()
        .flat_map(|plane| &plane.chunks)
        .all(|chunk| chunk.row_count == 16 && chunk.uncompressed_size == 1_600));

    let gray1 = compile_image(
        &source,
        CompileOptions {
            storage_format: StorageFormat::Gray1,
            dither: false,
        },
    )
    .unwrap();
    assert_eq!(gray1.pixel_format, 1);
    assert_eq!(gray1.planes.len(), 1);
    assert_eq!(gray1.planes[0].slot, 2);
    assert_eq!(gray1.planes[0].chunks.len(), 30);
    let mut builder = BookBuilder::new(BookConfig::xteink_x4_gray1());
    builder.add_page(gray1);
    let mut bytes = Cursor::new(Vec::new());
    builder.write_to(&mut bytes).unwrap();
    assert_eq!(
        decode_book_page(&bytes.into_inner(), 0)
            .unwrap()
            .pixel_format,
        binbook_core::PixelFormat::Gray1Packed
    );
}

#[test]
fn decodes_none_packbits_lz4_and_rejects_out_of_range_book_pages() {
    let original = (0_u8..=255).cycle().take(9_001).collect::<Vec<_>>();
    let mut decoded = vec![0_u8; original.len()];
    decode_blob(CompressionMethod::None, &original, &mut decoded).unwrap();
    assert_eq!(decoded, original);

    let packbits = binbook_compress::encode(&original);
    decode_blob(CompressionMethod::RlePackBits, &packbits, &mut decoded).unwrap();
    assert_eq!(decoded, original);

    let lz4 = lz4_flex::block::compress(&original);
    decode_blob(CompressionMethod::Lz4, &lz4, &mut decoded).unwrap();
    assert_eq!(decoded, original);

    let page = compile_image(&gradient_png(8, 8), CompileOptions::default()).unwrap();
    let mut builder = BookBuilder::new(BookConfig::xteink_x4());
    builder.add_page(page);
    let mut book_bytes = Cursor::new(Vec::new());
    builder.write_to(&mut book_bytes).unwrap();
    let book_bytes = book_bytes.into_inner();
    let decoded_page = decode_book_page(&book_bytes, 0).unwrap();
    assert_eq!((decoded_page.width, decoded_page.height), (800, 480));
    let png = encode_page_png(&decoded_page).unwrap();
    let decoded_png = image::load_from_memory_with_format(&png, ImageFormat::Png).unwrap();
    assert_eq!((decoded_png.width(), decoded_png.height()), (800, 480));
    assert!(decode_book_page(&book_bytes, 1).is_err());

    let mut scratch = [0_u8; 1024];
    let mut book = Book::open(SliceSource::new(&book_bytes), &mut scratch).unwrap();
    let page = book
        .page(book.page_number(0).unwrap(), &mut scratch)
        .unwrap();
    assert!(page.planes.get(PlaneSlot::OverlayMsb).is_some());
}

#[test]
fn exact_size_corner_pixels_keep_x4_orientation_and_levels() {
    let mut source = ImageBuffer::from_pixel(
        u32::from(LOGICAL_WIDTH),
        u32::from(LOGICAL_HEIGHT),
        Luma([255_u8]),
    );
    for (x, y, value) in [
        (0, 0, 0),
        (LOGICAL_WIDTH - 1, 0, 85),
        (0, LOGICAL_HEIGHT - 1, 170),
        (LOGICAL_WIDTH - 1, LOGICAL_HEIGHT - 1, 255),
    ] {
        source.put_pixel(u32::from(x), u32::from(y), Luma([value]));
    }
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(source)
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap();
    let page = compile_image(
        &encoded.into_inner(),
        CompileOptions {
            storage_format: StorageFormat::Gray2,
            dither: false,
        },
    )
    .unwrap();
    let mut builder = BookBuilder::new(BookConfig::xteink_x4());
    builder.add_page(page);
    let mut bytes = Cursor::new(Vec::new());
    builder.write_to(&mut bytes).unwrap();
    let decoded = decode_book_page(&bytes.into_inner(), 0).unwrap();
    for (logical_x, logical_y, expected) in [
        (0, 0, 0),
        (LOGICAL_WIDTH - 1, 0, 1),
        (0, LOGICAL_HEIGHT - 1, 2),
        (LOGICAL_WIDTH - 1, LOGICAL_HEIGHT - 1, 3),
    ] {
        let (x, y) = logical_to_physical(logical_x, logical_y);
        let index = usize::from(y) * usize::from(decoded.width) + usize::from(x);
        let level = (decoded.packed[index / 4] >> (6 - (index % 4) * 2)) & 3;
        assert_eq!(level, expected);
    }
}
