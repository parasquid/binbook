use std::default::Default;

use binbook_document::{
    resolve_resource_path, BlockKind, ComputedStyle, Diagnostic, DiagnosticCode, DisplayMode,
    FontStyle, FontWeight, InlineKind, Node, Resource,
};
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::css::{merge_patch, parse_css, parse_inline, Stylesheet};
use crate::EpubError;

pub(crate) fn parse_html(
    path: &str,
    source: &str,
    base_styles: &Stylesheet,
    resources: &[Resource],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Node, EpubError> {
    let dom = parse_document(RcDom::default(), Default::default()).one(source);
    let mut styles = base_styles.clone();
    for internal in collect_style_text(&dom.document) {
        styles.extend(parse_css(&internal, path));
    }
    diagnostics.extend(styles.diagnostics.iter().cloned());
    let style = ComputedStyle {
        display: DisplayMode::Block,
        ..ComputedStyle::default()
    };
    let children = child_nodes(&dom.document, path, &style, &styles, resources, diagnostics);
    Ok(Node::block(BlockKind::Document, style, children))
}

fn child_nodes(
    handle: &Handle,
    path: &str,
    parent_style: &ComputedStyle,
    stylesheet: &Stylesheet,
    resources: &[Resource],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Node> {
    handle
        .children
        .borrow()
        .iter()
        .filter_map(|child| {
            convert(
                child,
                path,
                parent_style,
                stylesheet,
                resources,
                diagnostics,
            )
        })
        .collect()
}

fn convert(
    handle: &Handle,
    path: &str,
    parent_style: &ComputedStyle,
    stylesheet: &Stylesheet,
    resources: &[Resource],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Node> {
    match &handle.data {
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let value = if parent_style.preserve_whitespace {
                text.to_string()
            } else {
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            (!value.is_empty()).then(|| Node::text(value))
        }
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref().to_ascii_lowercase();
            if matches!(
                tag.as_str(),
                "script" | "form" | "audio" | "video" | "style" | "link" | "head"
            ) {
                if !matches!(tag.as_str(), "style" | "link" | "head") {
                    diagnostics.push(Diagnostic::new(DiagnosticCode::UnsupportedElement, path, 0));
                }
                return None;
            }
            let attrs = attrs.borrow();
            let id = attr(&attrs, "id");
            let classes = attr(&attrs, "class").unwrap_or_default();
            let mut patch = defaults(&tag);
            merge_patch(
                &mut patch,
                &stylesheet.patch_for(&tag, id.as_deref(), &classes),
            );
            if let Some(inline) = attr(&attrs, "style") {
                merge_patch(&mut patch, &parse_inline(&inline, path, diagnostics));
            }
            let style = ComputedStyle::cascade(parent_style, &patch);
            if style.display == DisplayMode::None {
                return None;
            }
            if tag == "img" {
                let src = attr(&attrs, "src")?;
                let resolved = resolve_resource_path(path, &src).ok()?;
                let resource = resources.iter().find(|resource| resource.path == resolved);
                return match resource {
                    Some(resource) => Some(Node::image(
                        resource.id,
                        attr(&attrs, "alt").unwrap_or_default(),
                    )),
                    None => {
                        diagnostics.push(Diagnostic::new(DiagnosticCode::MissingResource, path, 0));
                        Some(Node::text(
                            attr(&attrs, "alt").unwrap_or_else(|| "[missing image]".into()),
                        ))
                    }
                };
            }
            if tag == "br" {
                return Some(Node::LineBreak);
            }
            if tag == "hr" {
                return Some(Node::HorizontalRule);
            }
            let mut children =
                child_nodes(handle, path, &style, stylesheet, resources, diagnostics);
            if let Some(id) = id {
                children.insert(0, Node::Anchor(id));
            }
            Some(match block_kind(&tag) {
                Some(kind) => Node::Block {
                    kind,
                    style,
                    children,
                },
                None => Node::Inline {
                    kind: inline_kind(&tag),
                    style,
                    children,
                },
            })
        }
        _ => {
            let children = child_nodes(
                handle,
                path,
                parent_style,
                stylesheet,
                resources,
                diagnostics,
            );
            (!children.is_empty()).then(|| Node::Block {
                kind: BlockKind::Generic,
                style: parent_style.clone(),
                children,
            })
        }
    }
}

fn collect_style_text(handle: &Handle) -> Vec<String> {
    let mut output = Vec::new();
    if let NodeData::Element { name, .. } = &handle.data {
        if name.local.as_ref().eq_ignore_ascii_case("style") {
            let text = handle
                .children
                .borrow()
                .iter()
                .filter_map(|child| match &child.data {
                    NodeData::Text { contents } => Some(contents.borrow().to_string()),
                    _ => None,
                })
                .collect::<String>();
            output.push(text);
        }
    }
    for child in handle.children.borrow().iter() {
        output.extend(collect_style_text(child));
    }
    output
}

fn attr(attrs: &[html5ever::Attribute], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|attr| attr.name.local.as_ref().eq_ignore_ascii_case(name))
        .map(|attr| attr.value.to_string())
}

fn defaults(tag: &str) -> binbook_document::StylePatch {
    let mut patch = binbook_document::StylePatch::default();
    if block_kind(tag).is_some() {
        patch.display = Some(DisplayMode::Block);
    }
    match tag {
        "h1" => {
            patch.font_size_milli_px = Some(48_000);
            patch.font_weight = Some(FontWeight::Bold);
        }
        "h2" => {
            patch.font_size_milli_px = Some(40_000);
            patch.font_weight = Some(FontWeight::Bold);
        }
        "h3" | "h4" | "h5" | "h6" => patch.font_weight = Some(FontWeight::Bold),
        "strong" | "b" => patch.font_weight = Some(FontWeight::Bold),
        "em" | "i" => patch.font_style = Some(FontStyle::Italic),
        "pre" => patch.preserve_whitespace = Some(true),
        _ => {}
    }
    patch
}

fn block_kind(tag: &str) -> Option<BlockKind> {
    Some(match tag {
        "html" | "body" | "div" | "section" | "article" | "main" => BlockKind::Generic,
        "p" => BlockKind::Paragraph,
        "h1" => BlockKind::Heading(1),
        "h2" => BlockKind::Heading(2),
        "h3" => BlockKind::Heading(3),
        "h4" => BlockKind::Heading(4),
        "h5" => BlockKind::Heading(5),
        "h6" => BlockKind::Heading(6),
        "ol" => BlockKind::List { ordered: true },
        "ul" => BlockKind::List { ordered: false },
        "li" => BlockKind::ListItem,
        "blockquote" => BlockKind::BlockQuote,
        "pre" => BlockKind::Preformatted,
        "figure" => BlockKind::Figure,
        "table" => BlockKind::Table,
        "tr" => BlockKind::TableRow,
        "td" | "th" => BlockKind::TableCell,
        _ => return None,
    })
}

fn inline_kind(tag: &str) -> InlineKind {
    match tag {
        "em" | "i" => InlineKind::Emphasis,
        "strong" | "b" => InlineKind::Strong,
        "a" => InlineKind::Link,
        "code" => InlineKind::Code,
        _ => InlineKind::Span,
    }
}
