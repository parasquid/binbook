use binbook_document::{
    BlockKind, ComputedStyle, Document, FontWeight, InlineKind, Node, Resource, ResourceId,
    SpineItem,
};
use binbook_render::{render_document, RenderOptions, WarningCode};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use std::io::Cursor;

#[test]
fn wraps_and_paginates_structural_content_deterministically() {
    let style = ComputedStyle::default();
    let page_break = ComputedStyle {
        break_before: true,
        ..style.clone()
    };
    let bold = ComputedStyle {
        font_weight: FontWeight::Bold,
        ..style.clone()
    };
    let root = Node::block(
        BlockKind::Document,
        style.clone(),
        vec![
            Node::block(
                BlockKind::Heading(1),
                style.clone(),
                vec![Node::text("Heading")],
            ),
            Node::block(
                BlockKind::Paragraph,
                style.clone(),
                vec![
                    Node::text("مرحبا soft\u{ad}hyphen styled text ".repeat(20)),
                    Node::Inline {
                        kind: InlineKind::Strong,
                        style: bold,
                        children: vec![Node::text("bold span")],
                    },
                ],
            ),
            Node::block(
                BlockKind::List { ordered: true },
                style.clone(),
                vec![Node::block(
                    BlockKind::ListItem,
                    style.clone(),
                    vec![Node::text("item")],
                )],
            ),
            Node::block(
                BlockKind::BlockQuote,
                style.clone(),
                vec![Node::text("quote")],
            ),
            Node::block(
                BlockKind::Preformatted,
                style.clone(),
                vec![Node::text("pre\n line")],
            ),
            Node::image(ResourceId(4), "illustration"),
            Node::block(
                BlockKind::Table,
                style.clone(),
                vec![Node::block(
                    BlockKind::TableRow,
                    style.clone(),
                    vec![
                        Node::block(
                            BlockKind::TableCell,
                            style.clone(),
                            vec![Node::text("left")],
                        ),
                        Node::block(
                            BlockKind::TableCell,
                            style.clone(),
                            vec![Node::text("right")],
                        ),
                    ],
                )],
            ),
            Node::block(
                BlockKind::Paragraph,
                page_break,
                vec![Node::text("second page")],
            ),
        ],
    );
    let document = Document {
        spine: vec![SpineItem {
            resource: ResourceId(0),
            linear: true,
            root,
        }],
        ..Document::default()
    };
    let first = render_document(&document, &RenderOptions::default()).unwrap();
    let second = render_document(&document, &RenderOptions::default()).unwrap();
    assert!(first.pages.len() >= 2);
    assert_eq!(first, second);
    assert!(first
        .pages
        .iter()
        .all(|page| page.stored_width == 800 && page.stored_height == 480));
    assert!(!first
        .warnings
        .iter()
        .any(|warning| warning.code == WarningCode::MissingGlyph));
}

#[test]
fn expands_portrait_and_landscape_artwork_and_recovers_unsupported_bytes() {
    // Given: text separated by portrait, landscape, and unsupported artwork.
    let root = Node::block(
        BlockKind::Document,
        ComputedStyle::default(),
        vec![
            Node::text("before"),
            Node::captioned_image(ResourceId(1), "portrait", "Portrait caption"),
            Node::image(ResourceId(2), "landscape"),
            Node::image(ResourceId(3), "broken art"),
            Node::text("after"),
        ],
    );
    let document = Document {
        resources: vec![
            resource(1, png(3, 5)),
            resource(2, png(5, 3)),
            resource(3, b"not an image".to_vec()),
        ],
        spine: vec![SpineItem {
            resource: ResourceId(0),
            linear: true,
            root,
        }],
        ..Document::default()
    };

    // When: the document is rendered through the real raster pipeline.
    let rendered = render_document(&document, &RenderOptions::default()).unwrap();

    // Then: portrait uses one page, landscape two, and fallback remains visible.
    assert_eq!(rendered.pages.len(), 6);
    assert!(rendered
        .warnings
        .iter()
        .any(|warning| warning.code == WarningCode::UnsupportedImage));
    assert!(rendered.pages.iter().all(|page| page.pixel_format == 2));
    assert_ne!(rendered.pages[2].planes, rendered.pages[3].planes);
}

fn resource(id: u32, bytes: Vec<u8>) -> Resource {
    Resource {
        id: ResourceId(id),
        path: format!("image-{id}.png"),
        media_type: "image/png".into(),
        bytes,
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = ImageBuffer::from_fn(width, height, |x, _| {
        if x < width.div_ceil(2) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
