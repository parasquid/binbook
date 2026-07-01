use std::collections::BTreeMap;

use binbook_document::{DiagnosticCode, Document, ResourceId};
use binbook_encode::{CompiledPage, UsedFont};

use crate::font::load_fonts;
use crate::pagination::{paginate, Pages};
use crate::raster::raster_page;
use crate::{RenderError, RenderWarning, WarningCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontMode {
    Preserve,
    Force { family: String, bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOptions {
    pub font_mode: FontMode,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_mode: FontMode::Preserve,
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
    let rendered_pages = layout
        .pages
        .iter()
        .map(|text| raster_page(text, &mut fonts.system))
        .collect::<Result<Vec<_>, _>>()?;
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
        anchors: layout.anchors,
        used_fonts: fonts.used,
        font_section_sha256: fonts.digest,
        warnings: layout.warnings,
    })
}
