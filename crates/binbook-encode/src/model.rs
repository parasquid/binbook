use binbook_core::{CompressionMethod, FontSourceKind, FontStyle};

pub(crate) use crate::error::{EncodeError, ModelError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookConfig {
    pub profile_id: String,
    pub device_family: String,
    pub device_model: String,
    pub logical_width: u16,
    pub logical_height: u16,
    pub physical_width: u16,
    pub physical_height: u16,
    pub rotation_degrees: i16,
    pub storage_pixel_format: u16,
    pub grayscale_levels: u16,
    pub framebuffer_bits_per_pixel: u8,
    pub waveform_hint: u16,
}

impl BookConfig {
    #[must_use]
    pub fn xteink_x4() -> Self {
        Self {
            profile_id: "xteink-x4-portrait".into(),
            device_family: "xteink".into(),
            device_model: "x4".into(),
            logical_width: 480,
            logical_height: 800,
            physical_width: 800,
            physical_height: 480,
            rotation_degrees: 270,
            storage_pixel_format: 2,
            grayscale_levels: 4,
            framebuffer_bits_per_pixel: 2,
            waveform_hint: 2,
        }
    }

    #[must_use]
    pub fn xteink_x4_gray1() -> Self {
        let mut config = Self::xteink_x4();
        config.storage_pixel_format = 1;
        config.grayscale_levels = 2;
        config.framebuffer_bits_per_pixel = 1;
        config
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BookMetadata {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub publisher: String,
    pub language: String,
    pub series_name: String,
    pub series_index_milli: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub source_type: u16,
    pub filename: String,
    pub package_identifier: String,
    pub file_size: u64,
    pub md5: [u8; 16],
    pub sha256: [u8; 32],
}

impl SourceIdentity {
    #[must_use]
    pub fn from_bytes(
        source_type: u16,
        filename: impl Into<String>,
        package_identifier: impl Into<String>,
        bytes: &[u8],
    ) -> Self {
        Self {
            source_type,
            filename: filename.into(),
            package_identifier: package_identifier.into(),
            file_size: bytes.len() as u64,
            md5: [0; 16],
            sha256: crate::hashing::sha256(bytes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontPolicyMode {
    Preserve,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontPolicy {
    pub mode: FontPolicyMode,
    pub forced_font_sha256: [u8; 32],
    pub family: String,
    pub source_path: String,
    pub renderer: String,
}

impl FontPolicy {
    #[must_use]
    pub fn preserve() -> Self {
        Self {
            mode: FontPolicyMode::Preserve,
            forced_font_sha256: [0; 32],
            family: String::new(),
            source_path: String::new(),
            renderer: "binbook-render".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedFont {
    pub source_kind: FontSourceKind,
    pub family: String,
    pub source_path: String,
    pub sha256: [u8; 32],
    pub face_index: u32,
    pub weight: u16,
    pub stretch_milli: u16,
    pub style: FontStyle,
    pub flags: u16,
}

impl UsedFont {
    #[must_use]
    pub fn epub(
        family: impl Into<String>,
        source_path: impl Into<String>,
        sha256: [u8; 32],
        face_index: u32,
        weight: u16,
    ) -> Self {
        Self {
            source_kind: FontSourceKind::Epub,
            family: family.into(),
            source_path: source_path.into(),
            sha256,
            face_index,
            weight,
            stretch_milli: 1_000,
            style: FontStyle::Normal,
            flags: 1,
        }
    }

    #[must_use]
    pub fn epub_from_bytes(
        family: impl Into<String>,
        source_path: impl Into<String>,
        bytes: &[u8],
        face_index: u32,
        weight: u16,
    ) -> Self {
        Self::epub(
            family,
            source_path,
            crate::hashing::sha256(bytes),
            face_index,
            weight,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledChunk {
    pub compressed: Vec<u8>,
    pub row_start: u16,
    pub row_count: u16,
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPlane {
    pub slot: u8,
    pub compression: CompressionMethod,
    pub chunks: Vec<CompiledChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPage {
    pub page_kind: u16,
    pub pixel_format: u16,
    pub stored_width: u16,
    pub stored_height: u16,
    pub source_spine_index: u32,
    pub chapter_nav_index: u32,
    pub planes: Vec<CompiledPlane>,
}

impl CompiledPage {
    #[must_use]
    pub fn new_gray2(stored_width: u16, stored_height: u16, planes: Vec<CompiledPlane>) -> Self {
        Self {
            page_kind: 2,
            pixel_format: 2,
            stored_width,
            stored_height,
            source_spine_index: u32::MAX,
            chapter_nav_index: u32::MAX,
            planes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationEntry {
    pub title: String,
    pub source_href: String,
    pub nav_type: u16,
    pub level: u16,
    pub source_spine_index: u32,
    pub target_page_number: u32,
    pub parent: Option<u32>,
    pub first_child: Option<u32>,
    pub next_sibling: Option<u32>,
}

impl NavigationEntry {
    #[must_use]
    pub fn chapter(title: impl Into<String>, target_page_number: u32) -> Self {
        Self {
            title: title.into(),
            source_href: String::new(),
            nav_type: 3,
            level: 0,
            source_spine_index: u32::MAX,
            target_page_number,
            parent: None,
            first_child: None,
            next_sibling: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    pub page_count: u32,
    pub output_bytes: u64,
}
