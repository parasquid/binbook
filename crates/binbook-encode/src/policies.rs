use binbook_core::StringRef;

use crate::hashing::{set_section_hash, sha256};
use crate::model::{
    BookConfig, BookMetadata, CompiledPage, FontPolicy, FontPolicyMode, ModelError, SourceIdentity,
};
use crate::profile_policies::{build_display, build_layout, build_requirements};
use crate::strings::StringTable;

pub(crate) struct PolicySections {
    pub display: Vec<u8>,
    pub layout: Vec<u8>,
    pub requirements: Vec<u8>,
    pub source: Vec<u8>,
    pub metadata: Vec<u8>,
    pub rendition: Vec<u8>,
    pub font: Vec<u8>,
    pub typography: Vec<u8>,
    pub image: Vec<u8>,
    pub compression: Vec<u8>,
    pub chrome: Vec<u8>,
}

pub(crate) fn build_policies(
    config: &BookConfig,
    metadata: &BookMetadata,
    source: &SourceIdentity,
    font_policy: &FontPolicy,
    font_index: &[u8],
    pages: &[CompiledPage],
    strings: &mut StringTable,
) -> Result<PolicySections, ModelError> {
    let (display, display_hash) = build_display(config, strings)?;
    let (layout, layout_hash) = build_layout(config);
    let requirements = build_requirements(config, pages)?;
    let source_bytes = source_identity(source, strings)?;
    let metadata_bytes = book_metadata(metadata, strings)?;
    let (font, font_hash) = font_policy_section(font_policy, font_index, strings)?;
    let (typography, typography_hash) = typography();
    let (image, image_hash) = image(config);
    let (compression, compression_hash) = compression();
    let (chrome, chrome_hash) = chrome();
    let rendition = rendition(
        source.sha256,
        [
            display_hash,
            layout_hash,
            font_hash,
            typography_hash,
            image_hash,
            compression_hash,
            chrome_hash,
        ],
        strings.add("binbook-rust")?,
    );
    Ok(PolicySections {
        display,
        layout,
        requirements,
        source: source_bytes,
        metadata: metadata_bytes,
        rendition,
        font,
        typography,
        image,
        compression,
        chrome,
    })
}

fn source_identity(
    source: &SourceIdentity,
    strings: &mut StringTable,
) -> Result<Vec<u8>, ModelError> {
    let mut out = Vec::with_capacity(108);
    push_u16(&mut out, source.source_type);
    push_u16(&mut out, 0);
    push_u64(&mut out, source.file_size);
    out.extend_from_slice(&source.md5);
    out.extend_from_slice(&source.sha256);
    push_ref(&mut out, strings.add(&source.filename)?);
    push_ref(&mut out, strings.add(&source.package_identifier)?);
    out.resize(108, 0);
    Ok(out)
}

fn book_metadata(
    metadata: &BookMetadata,
    strings: &mut StringTable,
) -> Result<Vec<u8>, ModelError> {
    let mut out = Vec::with_capacity(88);
    for value in [
        &metadata.title,
        &metadata.subtitle,
        &metadata.author,
        &metadata.publisher,
        &metadata.language,
        &metadata.series_name,
    ] {
        push_ref(&mut out, strings.add(value)?);
    }
    push_u32(&mut out, metadata.series_index_milli);
    push_u32(&mut out, 0);
    out.resize(88, 0);
    Ok(out)
}

fn font_policy_section(
    policy: &FontPolicy,
    font_index: &[u8],
    strings: &mut StringTable,
) -> Result<(Vec<u8>, [u8; 32]), ModelError> {
    let mut out = Vec::with_capacity(124);
    let (mode, flags, digest) = match policy.mode {
        FontPolicyMode::Preserve if font_index.is_empty() => (1, 1 << 1, [0; 32]),
        FontPolicyMode::Preserve => (1, 1 << 1, sha256(font_index)),
        FontPolicyMode::Force => (2, 1, policy.forced_font_sha256),
    };
    push_u16(&mut out, mode);
    push_u16(&mut out, flags);
    out.extend_from_slice(&digest);
    push_ref(&mut out, strings.add(&policy.family)?);
    push_ref(&mut out, strings.add(&policy.source_path)?);
    push_ref(&mut out, strings.add(&policy.renderer)?);
    out.resize(124, 0);
    let hash = set_section_hash(&mut out, 60);
    Ok((out, hash))
}

fn typography() -> (Vec<u8>, [u8; 32]) {
    let mut out = Vec::with_capacity(108);
    for value in [24, 18, 0, 400] {
        push_u16(&mut out, value);
    }
    push_u32(&mut out, 1_000);
    push_u32(&mut out, 1_250);
    push_u16(&mut out, 0);
    push_u16(&mut out, 8);
    out.extend_from_slice(&0_i32.to_le_bytes());
    out.extend_from_slice(&0_i32.to_le_bytes());
    out.extend_from_slice(&[1, 1, 1, 0]);
    push_ref(&mut out, StringRef::default());
    push_u32(&mut out, 0);
    out.resize(108, 0);
    let hash = set_section_hash(&mut out, 44);
    (out, hash)
}

fn image(config: &BookConfig) -> (Vec<u8>, [u8; 32]) {
    let mut out = Vec::with_capacity(92);
    for value in [
        1,
        config.storage_pixel_format,
        1,
        1,
        1,
        1,
        config.grayscale_levels - 1,
        1_000,
        0,
        0,
        0,
        0,
    ] {
        push_u16(&mut out, value);
    }
    push_u32(&mut out, 0);
    out.resize(92, 0);
    let hash = set_section_hash(&mut out, 28);
    (out, hash)
}

fn compression() -> (Vec<u8>, [u8; 32]) {
    let mut out = Vec::with_capacity(78);
    push_u16(&mut out, 1);
    push_u32(&mut out, 1 << 1);
    push_u16(&mut out, 2);
    push_u16(&mut out, 16);
    push_u32(&mut out, 0);
    out.resize(78, 0);
    let hash = set_section_hash(&mut out, 14);
    (out, hash)
}

fn chrome() -> (Vec<u8>, [u8; 32]) {
    let mut out = vec![0_u8; 76];
    let hash = set_section_hash(&mut out, 12);
    (out, hash)
}

fn rendition(source_hash: [u8; 32], policy_hashes: [[u8; 32]; 7], compiler: StringRef) -> Vec<u8> {
    let mut canonical = Vec::with_capacity(256);
    canonical.extend_from_slice(&source_hash);
    for hash in policy_hashes {
        canonical.extend_from_slice(&hash);
    }
    let mut out = Vec::with_capacity(312);
    out.extend_from_slice(&sha256(&canonical));
    for hash in policy_hashes {
        out.extend_from_slice(&hash);
    }
    push_ref(&mut out, compiler);
    push_ref(&mut out, StringRef::default());
    push_u64(&mut out, 0);
    out.resize(312, 0);
    out
}

fn push_ref(out: &mut Vec<u8>, value: StringRef) {
    push_u32(out, value.offset);
    push_u32(out, value.length);
}
fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
