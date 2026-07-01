use binbook_document::{
    resolve_resource_path, BlockKind, ComputedStyle, Diagnostic, DiagnosticCode, Document,
    FontFace, FontStyle, FontWeight, NavItem, Node, Resource, ResourceId, StylePatch,
};

#[test]
fn models_typed_nodes_inherited_style_and_resolved_resources() {
    let parent = ComputedStyle {
        font_family: "Literata".into(),
        font_size_milli_px: 24_000,
        color_gray: 1,
        ..ComputedStyle::default()
    };
    let child = ComputedStyle::cascade(
        &parent,
        &StylePatch {
            font_weight: Some(FontWeight::Bold),
            margin_top_px: Some(8),
            ..StylePatch::default()
        },
    );
    assert_eq!(child.font_family, "Literata");
    assert_eq!(child.color_gray, 1);
    assert_eq!(child.font_weight, FontWeight::Bold);
    assert_eq!(child.margin_top_px, 8);

    let path = resolve_resource_path("OPS/chapters/ch1.xhtml", "../images/cover.png#hero").unwrap();
    assert_eq!(path, "OPS/images/cover.png");
    let node = Node::block(
        BlockKind::Paragraph,
        child,
        vec![Node::text("Hello"), Node::image(ResourceId(2), "cover")],
    );
    assert!(matches!(node, Node::Block { .. }));
}

#[test]
fn document_owns_navigation_fonts_resources_and_deterministic_diagnostics() {
    let mut document = Document::default();
    document.resources.push(Resource {
        id: ResourceId(0),
        path: "OPS/ch1.xhtml".into(),
        media_type: "application/xhtml+xml".into(),
        bytes: b"<p>Hello</p>".to_vec(),
    });
    document.navigation.push(NavItem {
        title: "Chapter".into(),
        resource: ResourceId(0),
        fragment: Some("start".into()),
        level: 0,
        parent: None,
    });
    document.fonts.push(FontFace {
        family: "Embedded".into(),
        resource: ResourceId(1),
        weight: FontWeight::Normal,
        style: FontStyle::Italic,
        obfuscated: true,
    });
    document.diagnostics.extend([
        Diagnostic::new(DiagnosticCode::UnsupportedCss, "z.css", 8),
        Diagnostic::new(DiagnosticCode::MissingResource, "a.xhtml", 2),
        Diagnostic::new(DiagnosticCode::UnsupportedCss, "a.xhtml", 1),
    ]);
    document.sort_diagnostics();
    assert_eq!(document.diagnostics[0].resource, "a.xhtml");
    assert_eq!(document.diagnostics[0].offset, 1);
    assert_eq!(document.diagnostics[1].offset, 2);
    assert_eq!(document.navigation[0].fragment.as_deref(), Some("start"));
    assert!(document.fonts[0].obfuscated);
}
