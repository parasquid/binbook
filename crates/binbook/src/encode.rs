use std::path::Path;

use binbook_compiler::{
    CompileEvent, CompileObserver, CompileOptions, CompileSource, FontFamily, NamedInput,
    ProfileId, StoragePixelFormat,
};

use crate::atomic_output::write_atomic;
use crate::input::{discover, DiscoveredInput};
use crate::{CliError, FontChoice, InputFormat, PixelFormat, Profile};

pub fn run_encode(
    input: &Path,
    output: &Path,
    input_format: InputFormat,
    profile: Profile,
    pixel_format: PixelFormat,
    no_dither: bool,
    font_family: Option<FontChoice>,
) -> Result<(), CliError> {
    let discovered = discover(input, input_format)?;
    for warning in &discovered.warnings {
        eprintln!("warning: {warning}");
    }
    let options = CompileOptions {
        profile: match profile {
            Profile::XteinkX4Portrait => ProfileId::XteinkX4Portrait,
        },
        pixel_format: match pixel_format {
            PixelFormat::Gray1 => StoragePixelFormat::Gray1,
            PixelFormat::Gray2 => StoragePixelFormat::Gray2,
        },
        dither: !no_dither,
        forced_font: font_family.map(|family| match family {
            FontChoice::Literata => FontFamily::Literata,
            FontChoice::OpenDyslexic | FontChoice::SansSerif => FontFamily::OpenDyslexic,
        }),
    };
    write_atomic(output, |target| {
        let mut observer = StderrObserver;
        match &discovered.input {
            DiscoveredInput::Images(inputs) => {
                let named = inputs
                    .iter()
                    .map(|input| NamedInput {
                        name: &input.name,
                        bytes: &input.bytes,
                    })
                    .collect::<Vec<_>>();
                binbook_compiler::compile(
                    CompileSource::ImageSequence(&named),
                    &options,
                    target,
                    &mut observer,
                )?;
            }
            DiscoveredInput::Epub(input) => {
                binbook_compiler::compile(
                    CompileSource::Epub(NamedInput {
                        name: &input.name,
                        bytes: &input.bytes,
                    }),
                    &options,
                    target,
                    &mut observer,
                )?;
            }
        }
        Ok(())
    })
}

struct StderrObserver;

impl CompileObserver for StderrObserver {
    fn on_event(&mut self, event: CompileEvent<'_>) {
        if let CompileEvent::Warning(warning) = event {
            eprintln!("warning[{:?}]: {}", warning.code, warning.message);
        }
    }
}
