use binbook_document::{BlockKind, ComputedStyle, Document, Node, Resource, ResourceId, SpineItem};
use binbook_render::{render_document, RenderOptions};
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
use std::io::Cursor;

#[test]
fn maps_anchors_and_forced_page_breaks_to_pages() {
    let break_style = ComputedStyle {
        break_before: true,
        ..ComputedStyle::default()
    };
    let root = Node::block(
        BlockKind::Document,
        ComputedStyle::default(),
        vec![
            Node::Anchor("start".into()),
            Node::block(
                BlockKind::Paragraph,
                ComputedStyle::default(),
                vec![Node::text("one")],
            ),
            Node::block(
                BlockKind::Paragraph,
                break_style,
                vec![Node::Anchor("second".into()), Node::text("two")],
            ),
        ],
    );
    let document = Document {
        spine: vec![SpineItem {
            resource: ResourceId(7),
            linear: true,
            root,
        }],
        ..Document::default()
    };
    let rendered = render_document(&document, &RenderOptions::default()).unwrap();
    assert_eq!(rendered.anchor_page(7, "start"), Some(0));
    assert_eq!(rendered.anchor_page(7, "second"), Some(1));
}

#[test]
fn remaps_anchor_after_landscape_artwork_to_two_expanded_pages() {
    // Given: anchors surround one landscape artwork layout page.
    let root = Node::block(
        BlockKind::Document,
        ComputedStyle::default(),
        vec![
            Node::Anchor("before".into()),
            Node::image(ResourceId(2), "landscape"),
            Node::Anchor("after".into()),
            Node::text("after"),
        ],
    );
    let document = Document {
        resources: vec![Resource {
            id: ResourceId(2),
            path: "landscape.png".into(),
            media_type: "image/png".into(),
            bytes: landscape_png(),
        }],
        spine: vec![SpineItem {
            resource: ResourceId(7),
            linear: true,
            root,
        }],
        ..Document::default()
    };

    // When: the renderer expands the artwork.
    let rendered = render_document(&document, &RenderOptions::default()).unwrap();

    // Then: the later anchor points after both artwork panels.
    assert_eq!(rendered.anchor_page(7, "before"), Some(0));
    assert_eq!(rendered.anchor_page(7, "after"), Some(2));
}

fn landscape_png() -> Vec<u8> {
    let image = ImageBuffer::from_pixel(5, 3, Luma([0_u8]));
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image)
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
