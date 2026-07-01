#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubError {
    InvalidContainer,
    InvalidPackage,
    MissingSpineResource,
    InvalidHtml,
    InvalidResource,
    DigitalRightsManagement,
}
