#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticCode {
    MissingResource,
    UnsupportedCss,
    UnsupportedElement,
    UnsupportedImage,
    UnsupportedFont,
    OversizedTableRow,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub resource: String,
    pub offset: u32,
    pub code: DiagnosticCode,
}

impl Diagnostic {
    #[must_use]
    pub fn new(code: DiagnosticCode, resource: impl Into<String>, offset: u32) -> Self {
        Self {
            resource: resource.into(),
            offset,
            code,
        }
    }
}
