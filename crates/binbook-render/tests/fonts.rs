use binbook_document::{
    BlockKind, ComputedStyle, Document, FontFace, FontStyle, FontWeight, Node, Resource,
    ResourceId, SpineItem,
};
use binbook_render::{render_document, FontMode, RenderOptions};

const LITERATA: &[u8] = include_bytes!("../../../binbook/assets/fonts/Literata/Literata.ttf");

fn document() -> Document {
    let style = ComputedStyle {
        font_family: "Used".into(),
        ..ComputedStyle::default()
    };
    Document {
        resources: vec![
            Resource {
                id: ResourceId(1),
                path: "used.ttf".into(),
                media_type: "font/ttf".into(),
                bytes: LITERATA.to_vec(),
            },
            Resource {
                id: ResourceId(2),
                path: "unused.ttf".into(),
                media_type: "font/ttf".into(),
                bytes: LITERATA.to_vec(),
            },
        ],
        fonts: vec![
            FontFace {
                family: "Used".into(),
                resource: ResourceId(1),
                weight: FontWeight::Normal,
                style: FontStyle::Normal,
                obfuscated: false,
            },
            FontFace {
                family: "Unused".into(),
                resource: ResourceId(2),
                weight: FontWeight::Normal,
                style: FontStyle::Normal,
                obfuscated: false,
            },
        ],
        spine: vec![SpineItem {
            resource: ResourceId(0),
            linear: true,
            root: Node::block(BlockKind::Paragraph, style, vec![Node::text("used face")]),
        }],
        ..Document::default()
    }
}

#[test]
fn records_only_rasterized_faces_and_forced_font_separately() {
    let preserved = render_document(&document(), &RenderOptions::default()).unwrap();
    assert_eq!(preserved.used_fonts.len(), 1);
    assert_eq!(preserved.used_fonts[0].family, "Used");
    assert_ne!(preserved.font_section_sha256, [0; 32]);

    let forced = render_document(
        &document(),
        &RenderOptions {
            font_mode: FontMode::Force {
                family: "Forced".into(),
                bytes: LITERATA.to_vec(),
            },
        },
    )
    .unwrap();
    assert_eq!(forced.used_fonts.len(), 1);
    assert_eq!(forced.used_fonts[0].family, "Forced");
}

#[test]
fn reports_missing_glyph_fallback_with_stable_context() {
    let mut input = document();
    input.spine[0].root = Node::block(
        BlockKind::Paragraph,
        ComputedStyle::default(),
        vec![Node::text("unassigned: \u{10fffd}")],
    );
    let rendered = render_document(&input, &RenderOptions::default()).unwrap();
    assert!(rendered
        .warnings
        .iter()
        .any(|warning| warning.code == binbook_render::WarningCode::MissingGlyph));
}
