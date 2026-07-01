use binbook_document::{Document, FontFace, FontWeight};
use binbook_encode::UsedFont;
use cosmic_text::FontSystem;
use sha2::{Digest, Sha256};

use crate::{FontMode, RenderError};

pub(crate) const FALLBACK_FAMILY: &str = "Literata";
pub(crate) const FALLBACK_BYTES: &[u8] =
    include_bytes!("../../../binbook/assets/fonts/Literata/Literata.ttf");

pub(crate) struct LoadedFonts {
    pub system: FontSystem,
    pub used: Vec<UsedFont>,
    pub digest: [u8; 32],
}

pub(crate) fn load_fonts(
    document: &Document,
    mode: &FontMode,
    families: &[String],
) -> Result<LoadedFonts, RenderError> {
    let mut system = FontSystem::new();
    let selected = match mode {
        FontMode::Force { family, bytes } => {
            vec![(family.clone(), "forced".into(), bytes.clone(), 400)]
        }
        FontMode::Preserve => selected_faces(document, families),
    };
    let selected = if selected.is_empty() {
        vec![(
            FALLBACK_FAMILY.into(),
            "bundled/Literata.ttf".into(),
            FALLBACK_BYTES.to_vec(),
            400,
        )]
    } else {
        selected
    };
    let mut used = Vec::new();
    let mut section = Vec::new();
    for (family, path, bytes, weight) in selected {
        if bytes.is_empty() {
            return Err(RenderError::InvalidFont);
        }
        system.db_mut().load_font_data(bytes.clone());
        section.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        section.extend_from_slice(&bytes);
        used.push(UsedFont::epub_from_bytes(family, path, &bytes, 0, weight));
    }
    used.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.sha256.cmp(&right.sha256))
    });
    Ok(LoadedFonts {
        system,
        used,
        digest: Sha256::digest(section).into(),
    })
}

fn selected_faces(document: &Document, families: &[String]) -> Vec<(String, String, Vec<u8>, u16)> {
    let mut output = document
        .fonts
        .iter()
        .filter(|face| families.iter().any(|family| family == &face.family))
        .filter_map(|face| font_tuple(document, face))
        .collect::<Vec<_>>();
    output.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    output.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    output
}

fn font_tuple(document: &Document, face: &FontFace) -> Option<(String, String, Vec<u8>, u16)> {
    let resource = document.resource(face.resource)?;
    let weight = match face.weight {
        FontWeight::Bold => 700,
        FontWeight::Numeric(value) => value,
        FontWeight::Normal => 400,
    };
    Some((
        face.family.clone(),
        resource.path.clone(),
        resource.bytes.clone(),
        weight,
    ))
}
