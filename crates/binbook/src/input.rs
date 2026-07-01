use std::fs;
use std::path::{Path, PathBuf};

use crate::error::io;
use crate::{CliError, InputFormat};

#[derive(Debug)]
pub(crate) struct OwnedInput {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum DiscoveredInput {
    Images(Vec<OwnedInput>),
    Epub(OwnedInput),
}

pub(crate) struct Discovery {
    pub input: DiscoveredInput,
    pub warnings: Vec<String>,
}

pub(crate) fn discover(path: &Path, requested: InputFormat) -> Result<Discovery, CliError> {
    if path.is_dir() {
        if requested == InputFormat::Epub {
            return Err(CliError::FormatMismatch);
        }
        return discover_directory(path);
    }
    let bytes = fs::read(path).map_err(|source| io(path, source))?;
    let actual = detect(&bytes);
    let expected = match requested {
        InputFormat::Auto => actual,
        InputFormat::Image => Some(InputKind::Image),
        InputFormat::Epub => Some(InputKind::Epub),
    };
    if actual != expected {
        return if actual.is_some() {
            Err(CliError::FormatMismatch)
        } else {
            Err(CliError::UnsupportedInput(path.into()))
        };
    }
    let name = utf8_name(path)?;
    let input = match actual {
        Some(InputKind::Image) => {
            binbook_image::decode_image(&bytes)
                .map_err(|_| CliError::UnsupportedInput(path.into()))?;
            DiscoveredInput::Images(vec![OwnedInput { name, bytes }])
        }
        Some(InputKind::Epub) => DiscoveredInput::Epub(OwnedInput { name, bytes }),
        None => return Err(CliError::UnsupportedInput(path.into())),
    };
    Ok(Discovery {
        input,
        warnings: Vec::new(),
    })
}

fn discover_directory(path: &Path) -> Result<Discovery, CliError> {
    let mut entries = fs::read_dir(path)
        .map_err(|source| io(path, source))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| io(path, source))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut images = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                warnings.push("skipping non-UTF-8 filename".into());
                continue;
            }
        };
        let bytes = fs::read(&entry_path).map_err(|source| io(&entry_path, source))?;
        if detect(&bytes) != Some(InputKind::Image) || binbook_image::decode_image(&bytes).is_err()
        {
            warnings.push(format!("skipping unsupported input {name}"));
            continue;
        }
        images.push(OwnedInput { name, bytes });
    }
    if images.is_empty() {
        return Err(CliError::NoEncodablePages);
    }
    Ok(Discovery {
        input: DiscoveredInput::Images(images),
        warnings,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputKind {
    Image,
    Epub,
}

fn detect(bytes: &[u8]) -> Option<InputKind> {
    if bytes.starts_with(b"PK\x03\x04")
        && bytes
            .windows(b"application/epub+zip".len())
            .any(|window| window == b"application/epub+zip")
    {
        Some(InputKind::Epub)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
        || std::str::from_utf8(bytes).ok().is_some_and(|text| {
            text.get(..text.len().min(1_024))
                .is_some_and(|prefix| prefix.contains("<svg"))
        })
    {
        Some(InputKind::Image)
    } else {
        None
    }
}

fn utf8_name(path: &Path) -> Result<String, CliError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| CliError::UnsupportedInput(PathBuf::from(path)))
}
