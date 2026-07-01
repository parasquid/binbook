use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "binbook")]
#[command(about = "CLI tool for BinBook compilation, inspection, decoding, and device diagnostics")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(after_help = "Example: binbook encode book.epub -o book.binbook")]
    Encode {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
        input_format: InputFormat,
        #[arg(long, value_enum, default_value_t = Profile::XteinkX4Portrait)]
        profile: Profile,
        #[arg(long, value_enum, default_value_t = PixelFormat::Gray2)]
        pixel_format: PixelFormat,
        #[arg(long)]
        no_dither: bool,
        #[arg(long, value_enum)]
        font_family: Option<FontChoice>,
    },
    Decode {
        book: PathBuf,
        #[arg(long)]
        page: u32,
        #[arg(short, long)]
        output: PathBuf,
    },
    Inspect {
        book: PathBuf,
        #[arg(long)]
        validate: bool,
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        json: bool,
    },
    Flash {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        firmware: PathBuf,
    },
    Upload {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        file: PathBuf,
        #[arg(short, long)]
        name: String,
    },
    List {
        #[arg(short, long)]
        port: String,
    },
    Delete {
        #[arg(short, long)]
        port: String,
        #[arg(short, long)]
        name: String,
    },
    #[command(subcommand)]
    Diag(DiagCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    Auto,
    Image,
    Epub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Profile {
    #[value(name = "xteink-x4-portrait")]
    XteinkX4Portrait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PixelFormat {
    Gray2,
    Gray1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FontChoice {
    Literata,
    OpenDyslexic,
    #[value(name = "sans-serif")]
    SansSerif,
}

#[derive(Subcommand)]
pub enum DiagCommand {
    Hello {
        #[arg(short, long)]
        port: String,
    },
    Key {
        #[arg(short, long)]
        port: String,
        #[arg(value_parser = ["LEFT", "RIGHT", "UP", "DOWN", "SELECT", "BACK", "POWER"])]
        key: String,
    },
    Page {
        #[arg(short, long)]
        port: String,
        #[command(subcommand)]
        action: PageAction,
    },
    Status {
        #[arg(short, long)]
        port: String,
    },
    Logs {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        since: Option<u32>,
        #[arg(long)]
        clear: bool,
    },
    Crash {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        clear: bool,
    },
    Probe {
        #[arg(short, long)]
        port: String,
        #[command(subcommand)]
        probe: ProbeCommand,
    },
    Exercise {
        #[command(subcommand)]
        exercise: ExerciseCommand,
    },
}

#[derive(Subcommand)]
pub enum ExerciseCommand {
    StagedGray {
        #[arg(short, long)]
        port: String,
    },
    NavBurst {
        #[arg(short, long)]
        port: String,
        #[arg(long, default_value_t = 10, value_parser = parse_rounds)]
        rounds: u16,
        #[arg(long, default_value_t = 0)]
        inter_key_ms: u64,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn parse_rounds(value: &str) -> Result<u16, String> {
    let rounds = value
        .parse::<u16>()
        .map_err(|error| format!("invalid rounds: {error}"))?;
    if (1..=100).contains(&rounds) {
        Ok(rounds)
    } else {
        Err("rounds must be between 1 and 100".into())
    }
}

#[derive(Subcommand)]
pub enum PageAction {
    Next,
    Previous,
    First,
    Last,
    Current,
    Goto { page: u32 },
}

#[derive(Subcommand)]
pub enum ProbeCommand {
    WindowCorners,
    ClearWhite,
    FullRefreshCurrent,
}
