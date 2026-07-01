use std::io::{Cursor, Seek, SeekFrom, Write};

use binbook_core::{validate_all, SliceSource, ValidationIssue, ValidationVisitor};
use binbook_encode::{BookBuilder, BookMetadata, FontPolicy, NavigationEntry, SourceIdentity};
use binbook_image::{compile_image, CompileOptions as ImageOptions};
use binbook_render::{render_document, RenderOptions};

use crate::support::{
    config, font_policy, forced_font, identity_for_images, storage, warning_code, warning_message,
};

use crate::{
    CompileError, CompileEvent, CompileObserver, CompileOptions, CompilePhase, CompileSource,
    CompileSummary, CompileWarning, SourceFormat,
};

pub fn compile<W: Write + Seek>(
    source: CompileSource<'_>,
    options: &CompileOptions,
    output: &mut W,
    observer: &mut impl CompileObserver,
) -> Result<CompileSummary, CompileError> {
    progress(observer, CompilePhase::ReadSource);
    let (builder, warnings, source_format) = match source {
        CompileSource::ImageSequence(inputs) => compile_images(inputs, options, observer)?,
        CompileSource::Epub(input) => compile_epub(input, options, observer)?,
    };
    for warning in &warnings {
        observer.on_event(CompileEvent::Warning(warning));
    }
    progress(observer, CompilePhase::Assemble);
    let mut staged = Cursor::new(Vec::new());
    let written = builder
        .write_to(&mut staged)
        .map_err(|_| CompileError::Assemble)?;
    progress(observer, CompilePhase::Validate);
    let bytes = staged.into_inner();
    validate(&bytes)?;
    output
        .seek(SeekFrom::Start(0))
        .map_err(CompileError::Output)?;
    output.write_all(&bytes).map_err(CompileError::Output)?;
    output.flush().map_err(CompileError::Output)?;
    Ok(CompileSummary {
        page_count: written.page_count,
        warning_count: warnings.len() as u32,
        output_bytes: bytes.len() as u64,
        source_format,
        pixel_format: options.pixel_format,
    })
}

fn compile_images(
    inputs: &[crate::NamedInput<'_>],
    options: &CompileOptions,
    observer: &mut impl CompileObserver,
) -> Result<(BookBuilder, Vec<CompileWarning>, SourceFormat), CompileError> {
    if inputs.is_empty() {
        return Err(CompileError::EmptySource);
    }
    progress(observer, CompilePhase::Parse);
    progress(observer, CompilePhase::Layout);
    progress(observer, CompilePhase::Rasterize);
    let mut builder = BookBuilder::new(config(options));
    let identity = identity_for_images(inputs);
    builder.set_source(SourceIdentity::from_bytes(
        2,
        "image-sequence",
        "",
        &identity,
    ));
    builder.set_font_policy(FontPolicy::preserve());
    for input in inputs {
        let page = compile_image(
            input.bytes,
            ImageOptions {
                storage_format: storage(options.pixel_format),
                dither: options.dither,
            },
        )
        .map_err(|_| CompileError::Image)?;
        builder.add_page(page);
    }
    progress(observer, CompilePhase::Compress);
    Ok((builder, Vec::new(), SourceFormat::ImageSequence))
}

fn compile_epub(
    input: crate::NamedInput<'_>,
    options: &CompileOptions,
    observer: &mut impl CompileObserver,
) -> Result<(BookBuilder, Vec<CompileWarning>, SourceFormat), CompileError> {
    progress(observer, CompilePhase::Parse);
    let parsed = binbook_epub::parse_epub(input.bytes).map_err(|_| CompileError::Epub)?;
    progress(observer, CompilePhase::Layout);
    let font_mode = forced_font(options.forced_font);
    let rendered = render_document(&parsed.document, &RenderOptions { font_mode })
        .map_err(|_| CompileError::Render)?;
    progress(observer, CompilePhase::Rasterize);
    let mut builder = BookBuilder::new(config(options));
    builder.set_metadata(BookMetadata {
        title: parsed.document.metadata.title.clone(),
        author: parsed.document.metadata.author.clone(),
        language: parsed.document.metadata.language.clone(),
        ..BookMetadata::default()
    });
    builder.set_source(SourceIdentity::from_bytes(
        1,
        input.name,
        &parsed.document.metadata.identifier,
        input.bytes,
    ));
    builder.set_font_policy(font_policy(options.forced_font));
    for page in rendered.pages {
        builder.add_page(page);
    }
    for font in rendered.used_fonts {
        builder.add_font(font);
    }
    add_navigation(&mut builder, &parsed.document, &rendered.anchors);
    progress(observer, CompilePhase::Compress);
    let warnings = rendered
        .warnings
        .into_iter()
        .map(|warning| CompileWarning {
            code: warning_code(warning.code),
            message: warning_message(warning.code).into(),
            resource: Some(warning.resource.0),
            spine_index: Some(warning.spine_index),
        })
        .collect();
    Ok((builder, warnings, SourceFormat::Epub))
}

fn add_navigation(
    builder: &mut BookBuilder,
    document: &binbook_document::Document,
    anchors: &std::collections::BTreeMap<(binbook_document::ResourceId, String), u32>,
) {
    for (index, item) in document.navigation.iter().enumerate() {
        let target = item
            .fragment
            .as_ref()
            .and_then(|fragment| anchors.get(&(item.resource, fragment.clone())))
            .copied()
            .unwrap_or(0);
        let first_child = document
            .navigation
            .iter()
            .position(|candidate| candidate.parent == Some(index as u32))
            .map(|value| value as u32);
        let next_sibling = document
            .navigation
            .iter()
            .enumerate()
            .skip(index + 1)
            .find_map(|(candidate_index, candidate)| {
                (candidate.parent == item.parent).then_some(candidate_index as u32)
            });
        builder.add_navigation(NavigationEntry {
            title: item.title.clone(),
            source_href: document
                .resource(item.resource)
                .map_or_else(String::new, |resource| resource.path.clone()),
            nav_type: 3,
            level: item.level,
            source_spine_index: document
                .spine
                .iter()
                .position(|spine| spine.resource == item.resource)
                .map_or(u32::MAX, |value| value as u32),
            target_page_number: target,
            parent: item.parent,
            first_child,
            next_sibling,
        });
    }
}

fn progress(observer: &mut impl CompileObserver, phase: CompilePhase) {
    observer.on_event(CompileEvent::Progress {
        phase,
        completed: 1,
        total: 1,
    });
}

#[derive(Default)]
struct Issues(Vec<ValidationIssue>);
impl ValidationVisitor for Issues {
    fn visit(&mut self, issue: ValidationIssue) {
        self.0.push(issue);
    }
}

fn validate(bytes: &[u8]) -> Result<(), CompileError> {
    let mut issues = Issues::default();
    validate_all(
        SliceSource::new(bytes),
        &mut [0; 1024],
        &mut [0; 256],
        &mut issues,
    )
    .map_err(|_| CompileError::Validate)?;
    if issues.0.is_empty() {
        Ok(())
    } else {
        Err(CompileError::Validate)
    }
}
