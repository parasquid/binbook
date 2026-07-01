use binbook_document::{BlockKind, ComputedStyle, Document, Node, ResourceId, SpineItem};
use binbook_render::{render_document, RenderOptions};

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
