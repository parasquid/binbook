use binbook_document::{BlockKind, DiagnosticCode, DisplayMode, FontWeight, Node, ResourceId};
use binbook_epub::{parse_epub, EpubError, EpubVersion};

mod fixture;

#[test]
fn parses_epub3_metadata_resources_spine_navigation_css_and_obfuscated_font() {
    let fixture = fixture::epub3();
    let parsed = parse_epub(&fixture.bytes).unwrap();
    assert_eq!(parsed.version, EpubVersion::Epub3);
    assert_eq!(parsed.document.metadata.title, "Synthetic Three");
    assert_eq!(parsed.document.metadata.author, "Ada Reader");
    assert_eq!(parsed.document.metadata.language, "en");
    assert_eq!(
        parsed.document.metadata.identifier,
        "urn:uuid:12345678-1234-1234-1234-123456789abc"
    );
    assert_eq!(parsed.document.spine.len(), 1);
    assert_ne!(parsed.source_sha256, [0; 32]);
    assert!(parsed.document.spine[0].linear);
    assert_eq!(parsed.document.navigation[0].title, "Start");
    assert_eq!(parsed.document.navigation[1].title, "Details");
    assert_eq!(parsed.document.navigation[1].level, 1);
    assert_eq!(parsed.document.navigation[1].parent, Some(0));
    assert_eq!(
        parsed.document.navigation[0].fragment.as_deref(),
        Some("start")
    );
    assert!(parsed
        .document
        .resources
        .iter()
        .any(|resource| resource.path == "OPS/images/pixel.png"));
    for media_type in [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/svg+xml",
        "font/otf",
        "font/ttf",
        "font/woff",
        "font/woff2",
    ] {
        assert!(parsed
            .document
            .resources
            .iter()
            .any(|resource| resource.media_type == media_type));
    }
    assert_eq!(parsed.document.fonts.len(), 1);
    let font = parsed
        .document
        .resource(parsed.document.fonts[0].resource)
        .unwrap();
    assert_eq!(font.bytes, fixture.font_bytes);
    assert!(parsed.document.fonts[0].obfuscated);
    assert!(parsed
        .document
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsupportedCss));
    assert!(parsed
        .document
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingResource));

    let root = &parsed.document.spine[0].root;
    let paragraph = find_block(root, BlockKind::Paragraph).expect("paragraph");
    let Node::Block {
        style, children, ..
    } = paragraph
    else {
        unreachable!()
    };
    assert_eq!(style.font_size_milli_px, 21_000);
    assert_eq!(style.font_weight, FontWeight::Bold);
    assert!(children
        .iter()
        .any(|node| matches!(node, Node::Image { .. })));
    assert!(!contains_text(root, "hidden text"));
    assert!(contains_text(root, "missing art"));
    assert_ne!(style.display, DisplayMode::None);

    let images = collect_images(root);
    assert_eq!(images.len(), 3);
    assert_eq!(images[0].1, "pixel");
    assert_eq!(images[0].2, None);
    assert_eq!(images[1].1, "Rat and Mole");
    assert_eq!(images[1].2, Some("THE RIVER BANK"));
    assert_eq!(images[2].1, "");
    assert_eq!(images[2].2, None);
    assert!(!contains_text(root, "THE RIVER BANK"));

    let resource_path = |id: ResourceId| parsed.document.resource(id).unwrap().path.as_str();
    assert_eq!(resource_path(images[1].0), "OPS/images/pixel.jpg");
    assert_eq!(resource_path(images[2].0), "OPS/images/pixel.webp");
}

#[test]
fn parses_epub2_ncx_and_rejects_drm_encryption() {
    let epub2 = parse_epub(&fixture::epub2()).unwrap();
    assert_eq!(epub2.version, EpubVersion::Epub2);
    assert_eq!(epub2.document.navigation[0].title, "Legacy Chapter");
    assert_eq!(
        epub2.document.navigation[0].fragment.as_deref(),
        Some("legacy")
    );

    assert!(matches!(
        parse_epub(&fixture::drm_epub()),
        Err(EpubError::DigitalRightsManagement)
    ));
}

fn find_block(node: &Node, kind: BlockKind) -> Option<&Node> {
    match node {
        Node::Block {
            kind: actual,
            children,
            ..
        } => {
            if *actual == kind {
                return Some(node);
            }
            children.iter().find_map(|child| find_block(child, kind))
        }
        Node::Inline { children, .. } => children.iter().find_map(|child| find_block(child, kind)),
        _ => None,
    }
}

fn contains_text(node: &Node, needle: &str) -> bool {
    match node {
        Node::Text(text) => text.contains(needle),
        Node::Block { children, .. } | Node::Inline { children, .. } => {
            children.iter().any(|child| contains_text(child, needle))
        }
        _ => false,
    }
}

fn collect_images(node: &Node) -> Vec<(ResourceId, &str, Option<&str>)> {
    match node {
        Node::Image {
            resource,
            alt,
            caption,
        } => vec![(*resource, alt, caption.as_deref())],
        Node::Block { children, .. } | Node::Inline { children, .. } => {
            children.iter().flat_map(collect_images).collect()
        }
        _ => Vec::new(),
    }
}
