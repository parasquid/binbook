use std::io::Cursor;

use binbook_document::{Document, DocumentMetadata, NavItem, Resource, ResourceId, SpineItem};
use rbook::epub::metadata::EpubVersion as RbookVersion;
use sha2::{Digest, Sha256};

use crate::css::{parse_css, Stylesheet};
use crate::fonts::{build_font_faces, deobfuscate, parse_encryption};
use crate::html::parse_html;
use crate::EpubError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubVersion {
    Epub2,
    Epub3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEpub {
    pub version: EpubVersion,
    pub source_sha256: [u8; 32],
    pub document: Document,
}

pub fn parse_epub(bytes: &[u8]) -> Result<ParsedEpub, EpubError> {
    let epub = rbook::Epub::options()
        .strict(false)
        .retain_variants(false)
        .read(Cursor::new(bytes.to_vec()))
        .map_err(|_| EpubError::InvalidContainer)?;
    let version = match epub.package().version() {
        RbookVersion::Epub2(_) => EpubVersion::Epub2,
        RbookVersion::Epub3(_) => EpubVersion::Epub3,
        _ => return Err(EpubError::InvalidPackage),
    };
    let metadata = epub.metadata();
    let identifier = metadata
        .identifier()
        .map(|value| value.value())
        .unwrap_or_default();
    let mut document = Document {
        metadata: DocumentMetadata {
            title: metadata
                .title()
                .map(|value| value.value())
                .unwrap_or_default()
                .into(),
            author: metadata
                .creators()
                .next()
                .map(|value| value.value())
                .unwrap_or_default()
                .into(),
            language: metadata
                .language()
                .map(|value| value.value())
                .unwrap_or_default()
                .into(),
            identifier: identifier.into(),
        },
        ..Document::default()
    };
    document.resources = read_resources(&epub)?;
    let encryption_source = epub.read_resource_str("/META-INF/encryption.xml").ok();
    let encryptions = parse_encryption(encryption_source.as_deref())?;
    deobfuscate(&mut document.resources, &encryptions, identifier);

    let mut stylesheet = Stylesheet::default();
    for resource in document
        .resources
        .iter()
        .filter(|resource| resource.media_type == "text/css")
    {
        if let Ok(source) = std::str::from_utf8(&resource.bytes) {
            stylesheet.extend(parse_css(source, &resource.path));
        }
    }
    document
        .diagnostics
        .extend(stylesheet.diagnostics.iter().cloned());
    document.fonts = build_font_faces(
        &mut document.resources,
        &stylesheet.fonts,
        &encryptions,
        &mut document.diagnostics,
    );
    document.spine = read_spine(
        &epub,
        &document.resources,
        &stylesheet,
        &mut document.diagnostics,
    )?;
    document.navigation = read_navigation(&epub, &document.resources);
    document.sort_diagnostics();
    Ok(ParsedEpub {
        version,
        source_sha256: Sha256::digest(bytes).into(),
        document,
    })
}

fn read_resources(epub: &rbook::Epub) -> Result<Vec<Resource>, EpubError> {
    epub.manifest()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            Ok(Resource {
                id: ResourceId(u32::try_from(index).map_err(|_| EpubError::InvalidPackage)?),
                path: normalize(entry.href().path().as_str()),
                media_type: entry.media_type().to_ascii_lowercase(),
                bytes: entry.read_bytes().map_err(|_| EpubError::InvalidResource)?,
            })
        })
        .collect()
}

fn read_spine(
    epub: &rbook::Epub,
    resources: &[Resource],
    stylesheet: &Stylesheet,
    diagnostics: &mut Vec<binbook_document::Diagnostic>,
) -> Result<Vec<SpineItem>, EpubError> {
    epub.spine()
        .iter()
        .map(|entry| {
            let manifest = entry
                .manifest_entry()
                .ok_or(EpubError::MissingSpineResource)?;
            let path = normalize(manifest.href().path().as_str());
            let resource = resources
                .iter()
                .find(|resource| resource.path == path)
                .ok_or(EpubError::MissingSpineResource)?;
            let source =
                std::str::from_utf8(&resource.bytes).map_err(|_| EpubError::InvalidHtml)?;
            Ok(SpineItem {
                resource: resource.id,
                linear: entry.is_linear(),
                root: parse_html(&path, source, stylesheet, resources, diagnostics)?,
            })
        })
        .collect()
}

fn read_navigation(epub: &rbook::Epub, resources: &[Resource]) -> Vec<NavItem> {
    let Some(root) = epub.toc().contents() else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut parents: Vec<u32> = Vec::new();
    for entry in root.flatten() {
        let Some(href) = entry.href() else { continue };
        let path = normalize(href.path().as_str());
        let Some(resource) = resources.iter().find(|resource| resource.path == path) else {
            continue;
        };
        let level = entry.depth().saturating_sub(1);
        parents.truncate(level);
        let parent = level
            .checked_sub(1)
            .and_then(|index| parents.get(index))
            .copied();
        let index = output.len() as u32;
        output.push(NavItem {
            title: entry.label().into(),
            resource: resource.id,
            fragment: href
                .fragment()
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            level: level as u16,
            parent,
        });
        parents.push(index);
    }
    output
}

fn normalize(path: &str) -> String {
    path.trim_start_matches('/').replace("%20", " ")
}
