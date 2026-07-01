use binbook_encode::{BookConfig, FontPolicy, FontPolicyMode};
use binbook_image::StorageFormat;
use binbook_render::{FontMode, WarningCode};

use crate::{CompileOptions, CompileWarningCode, FontFamily, NamedInput, StoragePixelFormat};

pub(crate) fn config(options: &CompileOptions) -> BookConfig {
    match options.pixel_format {
        StoragePixelFormat::Gray1 => BookConfig::xteink_x4_gray1(),
        StoragePixelFormat::Gray2 => BookConfig::xteink_x4(),
    }
}

pub(crate) fn storage(format: StoragePixelFormat) -> StorageFormat {
    match format {
        StoragePixelFormat::Gray1 => StorageFormat::Gray1,
        StoragePixelFormat::Gray2 => StorageFormat::Gray2,
    }
}

pub(crate) fn forced_font(family: Option<FontFamily>) -> FontMode {
    match family {
        None => FontMode::Preserve,
        Some(FontFamily::Literata) => FontMode::Force {
            family: "Literata".into(),
            bytes: include_bytes!("../../../binbook/assets/fonts/Literata/Literata.ttf").to_vec(),
        },
        Some(FontFamily::OpenDyslexic) => FontMode::Force {
            family: "OpenDyslexic".into(),
            bytes: include_bytes!(
                "../../../binbook/assets/fonts/OpenDyslexic/OpenDyslexic-Regular.otf"
            )
            .to_vec(),
        },
    }
}

pub(crate) fn font_policy(family: Option<FontFamily>) -> FontPolicy {
    match family {
        None => FontPolicy::preserve(),
        Some(value) => {
            let FontMode::Force { family, bytes } = forced_font(Some(value)) else {
                unreachable!()
            };
            FontPolicy {
                mode: FontPolicyMode::Force,
                forced_font_sha256: <sha2::Sha256 as sha2::Digest>::digest(&bytes).into(),
                family,
                source_path: "bundled".into(),
                renderer: "binbook-render".into(),
            }
        }
    }
}

pub(crate) fn identity_for_images(inputs: &[NamedInput<'_>]) -> Vec<u8> {
    let mut output = Vec::new();
    for input in inputs {
        output.extend_from_slice(input.name.as_bytes());
        output.push(0);
        output.extend_from_slice(input.bytes);
    }
    output
}

pub(crate) fn warning_code(code: WarningCode) -> CompileWarningCode {
    match code {
        WarningCode::MissingGlyph => CompileWarningCode::MissingGlyph,
        WarningCode::MissingResource => CompileWarningCode::MissingResource,
        WarningCode::OversizedTableRow => CompileWarningCode::OversizedTableRow,
        WarningCode::UnsupportedContent => CompileWarningCode::UnsupportedContent,
    }
}

pub(crate) fn warning_message(code: WarningCode) -> &'static str {
    match code {
        WarningCode::MissingGlyph => "missing glyph rendered with fallback",
        WarningCode::MissingResource => "missing resource rendered as fallback",
        WarningCode::OversizedTableRow => "oversized table row rendered sequentially",
        WarningCode::UnsupportedContent => "unsupported content degraded deterministically",
    }
}
