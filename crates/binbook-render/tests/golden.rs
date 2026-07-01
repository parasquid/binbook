use binbook_document::{
    BlockKind, ComputedStyle, Diagnostic, DiagnosticCode, Document, Node, ResourceId, SpineItem,
};
use binbook_encode::{BookBuilder, BookConfig, FontPolicy, SourceIdentity};
use binbook_render::{render_document, RenderOptions, WarningCode};
use sha2::{Digest, Sha256};
use std::io::Cursor;

#[test]
fn degrades_oversized_table_rows_with_stable_context() {
    let root = Node::block(
        BlockKind::Table,
        ComputedStyle::default(),
        vec![Node::block(
            BlockKind::TableRow,
            ComputedStyle::default(),
            vec![Node::text("row ".repeat(2_000))],
        )],
    );
    let document = Document {
        spine: vec![SpineItem {
            resource: ResourceId(9),
            linear: true,
            root,
        }],
        diagnostics: vec![Diagnostic::new(
            DiagnosticCode::UnsupportedCss,
            "chapter.xhtml",
            3,
        )],
        ..Document::default()
    };
    let rendered = render_document(&document, &RenderOptions::default()).unwrap();
    assert!(rendered.warnings.windows(2).all(|pair| pair[0] <= pair[1]));
    let warning = rendered
        .warnings
        .iter()
        .find(|warning| warning.code == WarningCode::OversizedTableRow)
        .unwrap();
    assert_eq!(warning.spine_index, 0);
    assert_eq!(warning.resource, ResourceId(9));

    let mut builder = BookBuilder::new(BookConfig::xteink_x4());
    builder.set_source(SourceIdentity::from_bytes(
        1,
        "golden.epub",
        "golden",
        b"golden",
    ));
    builder.set_font_policy(FontPolicy::preserve());
    builder.add_page(rendered.pages[0].clone());
    let mut bytes = Cursor::new(Vec::new());
    builder.write_to(&mut bytes).unwrap();
    let decoded = binbook_image::decode_book_page(&bytes.into_inner(), 0).unwrap();
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(&decoded.packed)),
        [
            136, 167, 75, 208, 44, 48, 198, 96, 147, 188, 10, 132, 32, 247, 20, 219, 197, 249, 192,
            145, 100, 117, 135, 155, 3, 167, 90, 72, 205, 150, 248, 37,
        ]
    );
}
