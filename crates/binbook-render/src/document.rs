use std::collections::BTreeMap;

use binbook_document::{DiagnosticCode, Document, ResourceId};
use binbook_encode::{CompiledPage, UsedFont};
use binbook_image::StorageFormat;

use crate::font::load_fonts;
use crate::pagination::{paginate, Pages};
use crate::raster::{raster_artwork, raster_page};
use crate::{RenderError, RenderWarning, WarningCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontMode {
    Preserve,
    Force { family: String, bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOptions {
    pub font_mode: FontMode,
    pub storage_format: StorageFormat,
    pub dither: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_mode: FontMode::Preserve,
            storage_format: StorageFormat::Gray2,
            dither: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDocument {
    pub pages: Vec<CompiledPage>,
    pub anchors: BTreeMap<(ResourceId, String), u32>,
    pub used_fonts: Vec<UsedFont>,
    pub font_section_sha256: [u8; 32],
    pub warnings: Vec<RenderWarning>,
}

impl RenderedDocument {
    #[must_use]
    pub fn anchor_page(&self, resource: u32, fragment: &str) -> Option<u32> {
        self.anchors
            .get(&(ResourceId(resource), fragment.into()))
            .copied()
    }
}

pub fn render_document(
    document: &Document,
    options: &RenderOptions,
) -> Result<RenderedDocument, RenderError> {
    let mut layout = Pages::default();
    for (index, spine) in document
        .spine
        .iter()
        .enumerate()
        .filter(|(_, item)| item.linear)
    {
        paginate(&spine.root, spine.resource, index as u32, &mut layout);
    }
    if layout.pages.is_empty() {
        return Err(RenderError::EmptyDocument);
    }
    let mut fonts = load_fonts(document, &options.font_mode, &layout.families)?;
    while layout.pages.len() > 1
        && layout
            .pages
            .last()
            .is_some_and(|page| page.spans.is_empty() && page.artwork.is_none())
    {
        layout.pages.pop();
    }
    let mut rendered_pages = Vec::new();
    let mut output_starts = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        output_starts.push(rendered_pages.len() as u32);
        if let Some(artwork) = &page.artwork {
            let Some(resource) = document.resource(artwork.resource) else {
                continue;
            };
            match raster_artwork(
                resource,
                artwork.caption.as_deref(),
                &mut fonts.system,
                options,
            ) {
                Ok((pages, truncated)) => {
                    rendered_pages.extend(pages.into_iter().map(|page| (page, false)));
                    if truncated {
                        layout.warnings.push(RenderWarning {
                            resource: artwork.resource,
                            spine_index: artwork.spine_index,
                            offset: 0,
                            code: WarningCode::UnsupportedContent,
                        });
                    }
                }
                Err(_) => {
                    let fallback = crate::pagination::Page {
                        spans: vec![crate::pagination::Span {
                            text: format!("[image: {}]", artwork.alt),
                            style: Default::default(),
                        }],
                        ..Default::default()
                    };
                    rendered_pages
                        .push((raster_page(&fallback, &mut fonts.system, options)?.0, false));
                    layout.warnings.push(RenderWarning {
                        resource: artwork.resource,
                        spine_index: artwork.spine_index,
                        offset: 0,
                        code: WarningCode::UnsupportedImage,
                    });
                }
            }
        } else {
            rendered_pages.push(raster_page(page, &mut fonts.system, options)?);
        }
    }
    let resource = document
        .spine
        .first()
        .map_or(ResourceId(0), |item| item.resource);
    for (page_index, (_, missing)) in rendered_pages.iter().enumerate() {
        if *missing {
            layout.warnings.push(RenderWarning {
                resource,
                spine_index: 0,
                offset: page_index as u32,
                code: WarningCode::MissingGlyph,
            });
        }
    }
    let pages = rendered_pages.into_iter().map(|(page, _)| page).collect();
    let anchors = layout
        .anchors
        .iter()
        .filter_map(|(anchor, page)| {
            output_starts
                .get(*page as usize)
                .map(|output| (anchor.clone(), *output))
        })
        .collect();
    for diagnostic in &document.diagnostics {
        layout.warnings.push(RenderWarning {
            resource,
            spine_index: 0,
            offset: diagnostic.offset,
            code: match diagnostic.code {
                DiagnosticCode::MissingResource => WarningCode::MissingResource,
                DiagnosticCode::OversizedTableRow => WarningCode::OversizedTableRow,
                _ => WarningCode::UnsupportedContent,
            },
        });
    }
    layout.warnings.sort();
    Ok(RenderedDocument {
        pages,
        anchors,
        used_fonts: fonts.used,
        font_section_sha256: fonts.digest,
        warnings: layout.warnings,
    })
}
