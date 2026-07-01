use std::collections::BTreeMap;

use binbook_document::{BlockKind, ComputedStyle, Node, ResourceId};

use crate::{RenderWarning, WarningCode};

const CHARS_PER_PAGE: usize = 1_250;

#[derive(Debug, Default)]
pub(crate) struct Page {
    pub spans: Vec<Span>,
    len: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct Span {
    pub text: String,
    pub style: ComputedStyle,
}

#[derive(Debug, Default)]
pub(crate) struct Pages {
    pub pages: Vec<Page>,
    pub anchors: BTreeMap<(ResourceId, String), u32>,
    pub families: Vec<String>,
    pub warnings: Vec<RenderWarning>,
}

pub(crate) fn paginate(root: &Node, resource: ResourceId, spine_index: u32, pages: &mut Pages) {
    if pages.pages.is_empty() {
        pages.pages.push(Page::default());
    }
    visit(
        root,
        &ComputedStyle::default(),
        resource,
        spine_index,
        pages,
    );
}

fn visit(
    node: &Node,
    inherited: &ComputedStyle,
    resource: ResourceId,
    spine_index: u32,
    pages: &mut Pages,
) {
    match node {
        Node::Text(text) => push_text(text, inherited, pages),
        Node::Anchor(anchor) => {
            pages
                .anchors
                .insert((resource, anchor.clone()), page_index(pages));
        }
        Node::LineBreak => push_text("\n", inherited, pages),
        Node::HorizontalRule => push_text("\n────────\n", inherited, pages),
        Node::Image { alt, .. } => push_text(&format!("\n[image: {alt}]\n"), inherited, pages),
        Node::Inline {
            style, children, ..
        } => {
            record_family(&style.font_family, pages);
            for child in children {
                visit(child, style, resource, spine_index, pages);
            }
        }
        Node::Block {
            kind,
            style,
            children,
        } => {
            if style.break_before && pages.pages.last().is_some_and(|page| page.len > 0) {
                pages.pages.push(Page::default());
            }
            record_family(&style.font_family, pages);
            let oversized_row =
                matches!(kind, BlockKind::TableRow) && text_len(node) > CHARS_PER_PAGE;
            if oversized_row {
                pages.warnings.push(RenderWarning {
                    resource,
                    spine_index,
                    offset: 0,
                    code: WarningCode::OversizedTableRow,
                });
            }
            if matches!(kind, BlockKind::TableRow) && !oversized_row {
                push_equal_row(children, style, pages);
                return;
            }
            let prefix = match kind {
                BlockKind::ListItem => "• ",
                BlockKind::BlockQuote => "│ ",
                _ => "",
            };
            push_text(prefix, style, pages);
            for child in children {
                visit(child, style, resource, spine_index, pages);
            }
            if matches!(kind, BlockKind::TableCell) {
                push_text(" │ ", style, pages);
            } else if !matches!(
                kind,
                BlockKind::Document | BlockKind::Generic | BlockKind::TableRow | BlockKind::Table
            ) {
                push_text("\n\n", style, pages);
            }
            if matches!(kind, BlockKind::TableRow) {
                push_text("\n", style, pages);
            }
            if style.break_after {
                pages.pages.push(Page::default());
            }
        }
    }
}

fn push_text(mut value: &str, style: &ComputedStyle, pages: &mut Pages) {
    while !value.is_empty() {
        let remaining =
            CHARS_PER_PAGE.saturating_sub(pages.pages.last().map_or(0, |page| page.len));
        if remaining == 0 {
            pages.pages.push(Page::default());
            continue;
        }
        let split = floor_char_boundary(value, remaining.min(value.len()));
        let page = pages.pages.last_mut().expect("page exists");
        page.spans.push(Span {
            text: value[..split].into(),
            style: style.clone(),
        });
        page.len += split;
        value = &value[split..];
    }
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn text_len(node: &Node) -> usize {
    match node {
        Node::Text(text) => text.len(),
        Node::Block { children, .. } | Node::Inline { children, .. } => {
            children.iter().map(text_len).sum()
        }
        _ => 0,
    }
}

fn push_equal_row(children: &[Node], style: &ComputedStyle, pages: &mut Pages) {
    let cells = children.iter().map(plain_text).collect::<Vec<_>>();
    if cells.is_empty() {
        return;
    }
    let column_width = 72_usize.div_ceil(cells.len());
    let mut row = String::new();
    for cell in cells {
        let content = cell
            .chars()
            .take(column_width.saturating_sub(3))
            .collect::<String>();
        row.push_str(&format!(
            "{content:<width$}│ ",
            width = column_width.saturating_sub(2)
        ));
    }
    row.push('\n');
    push_text(&row, style, pages);
}

fn plain_text(node: &Node) -> String {
    match node {
        Node::Text(text) => text.clone(),
        Node::Block { children, .. } | Node::Inline { children, .. } => {
            children.iter().map(plain_text).collect()
        }
        Node::Image { alt, .. } => alt.clone(),
        Node::LineBreak => " ".into(),
        _ => String::new(),
    }
}

fn page_index(pages: &Pages) -> u32 {
    pages.pages.len().saturating_sub(1) as u32
}

fn record_family(family: &str, pages: &mut Pages) {
    if !pages.families.iter().any(|value| value == family) {
        pages.families.push(family.into());
    }
}
