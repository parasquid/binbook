use crate::{ComputedStyle, ResourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Document,
    Paragraph,
    Heading(u8),
    List { ordered: bool },
    ListItem,
    BlockQuote,
    Preformatted,
    Figure,
    Table,
    TableRow,
    TableCell,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    Span,
    Emphasis,
    Strong,
    Link,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Block {
        kind: BlockKind,
        style: ComputedStyle,
        children: Vec<Node>,
    },
    Inline {
        kind: InlineKind,
        style: ComputedStyle,
        children: Vec<Node>,
    },
    Text(String),
    Image {
        resource: ResourceId,
        alt: String,
    },
    LineBreak,
    HorizontalRule,
    Anchor(String),
}

impl Node {
    #[must_use]
    pub fn block(kind: BlockKind, style: ComputedStyle, children: Vec<Self>) -> Self {
        Self::Block {
            kind,
            style,
            children,
        }
    }

    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    #[must_use]
    pub fn image(resource: ResourceId, alt: impl Into<String>) -> Self {
        Self::Image {
            resource,
            alt: alt.into(),
        }
    }
}
