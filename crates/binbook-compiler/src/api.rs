#[derive(Debug, Clone, Copy)]
pub struct NamedInput<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub enum CompileSource<'a> {
    ImageSequence(&'a [NamedInput<'a>]),
    Epub(NamedInput<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileId {
    XteinkX4Portrait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePixelFormat {
    Gray1,
    Gray2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Literata,
    OpenDyslexic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    pub profile: ProfileId,
    pub pixel_format: StoragePixelFormat,
    pub dither: bool,
    pub forced_font: Option<FontFamily>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            profile: ProfileId::XteinkX4Portrait,
            pixel_format: StoragePixelFormat::Gray2,
            dither: true,
            forced_font: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilePhase {
    ReadSource,
    Parse,
    Layout,
    Rasterize,
    Compress,
    Assemble,
    Validate,
}

#[derive(Debug, Clone, Copy)]
pub enum CompileEvent<'a> {
    Progress {
        phase: CompilePhase,
        completed: u32,
        total: u32,
    },
    Warning(&'a CompileWarning),
}

pub trait CompileObserver {
    fn on_event(&mut self, event: CompileEvent<'_>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompileWarningCode {
    MissingGlyph,
    MissingResource,
    OversizedTableRow,
    UnsupportedContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileWarning {
    pub code: CompileWarningCode,
    pub message: String,
    pub resource: Option<u32>,
    pub spine_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    ImageSequence,
    Epub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileSummary {
    pub page_count: u32,
    pub warning_count: u32,
    pub output_bytes: u64,
    pub source_format: SourceFormat,
    pub pixel_format: StoragePixelFormat,
}
