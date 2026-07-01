use binbook_document::{
    BlockKind, ComputedStyle, Document, FontWeight, InlineKind, Node, ResourceId, SpineItem,
};
use binbook_render::{render_document, RenderOptions, WarningCode};

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
