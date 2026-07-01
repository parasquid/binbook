use binbook_document::{resolve_resource_path, Diagnostic, DiagnosticCode, FontFace, Resource};
use sha1::{Digest, Sha1};

use crate::css::FontFaceRule;
use crate::EpubError;

const IDPF_ALGORITHM: &str = "http://www.idpf.org/2008/embedding";
const ADOBE_ALGORITHM: &str = "http://ns.adobe.com/pdf/enc#RC";

#[derive(Debug, Clone)]
pub(crate) struct Encryption {
    path: String,
    algorithm: Algorithm,
}

#[derive(Debug, Clone, Copy)]
enum Algorithm {
    Idpf,
    Adobe,
}

pub(crate) fn parse_encryption(source: Option<&str>) -> Result<Vec<Encryption>, EpubError> {
    let Some(source) = source else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for segment in source.split("<EncryptedData").skip(1) {
        let segment = segment.split("</EncryptedData>").next().unwrap_or(segment);
        let algorithm =
            attribute_after(segment, "Algorithm=").ok_or(EpubError::DigitalRightsManagement)?;
        let path = attribute_after(segment, "URI=").ok_or(EpubError::InvalidPackage)?;
        let algorithm = match algorithm.as_str() {
            IDPF_ALGORITHM => Algorithm::Idpf,
            ADOBE_ALGORITHM => Algorithm::Adobe,
            _ => return Err(EpubError::DigitalRightsManagement),
        };
        entries.push(Encryption {
            path: path.trim_start_matches('/').into(),
            algorithm,
        });
    }
    Ok(entries)
}

pub(crate) fn deobfuscate(
    resources: &mut [Resource],
    encryptions: &[Encryption],
    identifier: &str,
) {
    for encryption in encryptions {
        let Some(resource) = resources
            .iter_mut()
            .find(|resource| resource.path == encryption.path)
        else {
            continue;
        };
        let (key, limit) = match encryption.algorithm {
            Algorithm::Idpf => {
                let normalized = identifier
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                (Sha1::digest(normalized.as_bytes()).to_vec(), 1_040)
            }
            Algorithm::Adobe => (adobe_key(identifier), 1_024),
        };
        if key.is_empty() {
            continue;
        }
        for (index, byte) in resource.bytes.iter_mut().take(limit).enumerate() {
            *byte ^= key[index % key.len()];
        }
    }
}

pub(crate) fn build_font_faces(
    resources: &mut [Resource],
    rules: &[FontFaceRule],
    encryptions: &[Encryption],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<FontFace> {
    let mut fonts = Vec::new();
    for rule in rules {
        let Ok(path) = resolve_resource_path(&rule.base, &rule.source) else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingResource,
                &rule.base,
                0,
            ));
            continue;
        };
        let Some(resource) = resources.iter_mut().find(|resource| resource.path == path) else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingResource,
                &rule.base,
                0,
            ));
            continue;
        };
        let decoded = match resource.bytes.get(..4) {
            Some(b"wOFF") => wuff::decompress_woff1(&resource.bytes).ok(),
            Some(b"wOF2") => wuff::decompress_woff2(&resource.bytes).ok(),
            _ => Some(resource.bytes.clone()),
        };
        let Some(decoded) = decoded else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnsupportedFont,
                &resource.path,
                0,
            ));
            continue;
        };
        resource.bytes = decoded;
        fonts.push(FontFace {
            family: rule.family.clone(),
            resource: resource.id,
            weight: rule.weight,
            style: rule.style,
            obfuscated: encryptions.iter().any(|entry| entry.path == path),
        });
    }
    fonts.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.resource.cmp(&right.resource))
    });
    fonts
}

fn attribute_after(source: &str, marker: &str) -> Option<String> {
    let tail = source.split(marker).nth(1)?.trim_start();
    let quote = tail.as_bytes().first().copied()? as char;
    matches!(quote, '\'' | '"')
        .then(|| tail[1..].split(quote).next().unwrap_or_default().to_owned())
}

fn adobe_key(identifier: &str) -> Vec<u8> {
    identifier
        .trim()
        .trim_start_matches("urn:uuid:")
        .chars()
        .filter(|character| *character != '-')
        .collect::<String>()
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|value| u8::from_str_radix(value, 16).ok())
        })
        .collect()
}
