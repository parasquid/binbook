use binbook_document::ResourceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarningCode {
    MissingGlyph,
    MissingResource,
    OversizedTableRow,
    UnsupportedContent,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RenderWarning {
    pub resource: ResourceId,
    pub spine_index: u32,
    pub offset: u32,
    pub code: WarningCode,
}
