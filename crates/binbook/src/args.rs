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
    Upload(UploadArgs),
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

#[derive(Debug, clap::Args)]
pub struct UploadArgs {
    #[arg(short, long)]
    pub port: String,
    #[arg(short, long)]
    pub file: PathBuf,
    #[arg(short, long, visible_alias = "path")]
    pub name: Option<String>,
    #[arg(long, value_enum)]
    pub progress: Option<ProgressMode>,
    #[arg(long)]
    pub no_wait_library: bool,
    #[arg(long)]
    pub json: bool,
    /// Atomically write a machine-readable transfer evidence report
    #[arg(long)]
    pub transfer_report: Option<PathBuf>,
    /// Capture every physical SD write transaction to JSON (diagnostic firmware only)
    #[arg(long)]
    pub sd_write_trace: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = RequestedTransferMode::Auto)]
    pub transfer_mode: RequestedTransferMode,
    #[arg(long, value_enum, default_value_t = RequestedTransferWindow::Auto)]
    pub window: RequestedTransferWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RequestedTransferMode {
    Auto,
    Legacy,
    Bulk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RequestedTransferWindow {
    Auto,
    #[value(name = "1")]
    One,
    #[value(name = "2")]
    Two,
    #[value(name = "4")]
    Four,
    #[value(name = "8")]
    Eight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProgressMode {
    Always,
    Auto,
    Never,
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
    RefreshMode {
        #[arg(short, long)]
        port: String,
        #[command(subcommand)]
        action: RefreshModeAction,
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
    #[command(subcommand)]
    Storage(StorageCommand),
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
    PageTurnCadence {
        #[arg(short, long)]
        port: String,
        #[arg(long, default_value = "DOWN", value_parser = parse_key_name)]
        key: String,
        #[arg(long, default_value_t = 0)]
        start_page: u32,
        #[arg(long, default_value_t = 8, value_parser = parse_positive_u16)]
        turns: u16,
        #[arg(long, default_value_t = 3, value_parser = parse_rounds)]
        rounds: u16,
        #[arg(long, default_value_t = 0, value_parser = parse_gap_ms)]
        min_gap_ms: u64,
        #[arg(long, default_value_t = 1_300, value_parser = parse_gap_ms)]
        max_gap_ms: u64,
        #[arg(long, default_value_t = 50, value_parser = parse_positive_u64)]
        coarse_step_ms: u64,
        #[arg(long, default_value_t = 10, value_parser = parse_positive_u64)]
        refine_step_ms: u64,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    PartialWindowBusy {
        #[arg(short, long)]
        port: String,
        #[arg(long, default_value_t = 3, value_parser = parse_rounds)]
        rounds: u16,
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

fn parse_positive_u16(value: &str) -> Result<u16, String> {
    let parsed = value
        .parse::<u16>()
        .map_err(|error| format!("invalid value: {error}"))?;
    if parsed == 0 {
        Err("value must be greater than zero".into())
    } else {
        Ok(parsed)
    }
}

fn parse_positive_u64(value: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|error| format!("invalid value: {error}"))?;
    if parsed == 0 {
        Err("value must be greater than zero".into())
    } else {
        Ok(parsed)
    }
}

fn parse_gap_ms(value: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|error| format!("invalid gap: {error}"))?;
    if parsed <= 3_000 {
        Ok(parsed)
    } else {
        Err("gap must be between 0 and 3000 ms".into())
    }
}

fn parse_key_name(value: &str) -> Result<String, String> {
    match value {
        "LEFT" | "RIGHT" | "UP" | "DOWN" => Ok(value.to_string()),
        _ => Err("key must be one of LEFT, RIGHT, UP, DOWN".into()),
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
pub enum RefreshModeAction {
    Get,
    Set {
        #[arg(value_enum)]
        mode: RefreshModeChoice,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RefreshModeChoice {
    Balanced,
    Fast,
}

#[derive(Subcommand)]
pub enum ProbeCommand {
    WindowCorners,
    ClearWhite,
    FullRefreshCurrent,
    PartialWindowBusy,
    CustomLutFastA,
    CustomLutFastB,
}

#[derive(Subcommand)]
pub enum StorageCommand {
    /// List files at a path on a storage backend
    List {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        path: Option<String>,
    },
    /// Read a file from a storage backend
    Read {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        path: String,
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
        #[arg(long)]
        offset: Option<u32>,
        #[arg(long)]
        max_bytes: Option<u32>,
        #[arg(long, value_enum, conflicts_with_all = ["offset", "max_bytes"])]
        progress: Option<ProgressMode>,
        #[arg(long, value_enum, default_value_t = RequestedTransferMode::Auto, conflicts_with_all = ["offset", "max_bytes"])]
        transfer_mode: RequestedTransferMode,
        #[arg(long, value_enum, default_value_t = RequestedTransferWindow::Auto, conflicts_with_all = ["offset", "max_bytes"])]
        window: RequestedTransferWindow,
        /// Atomically write a machine-readable transfer evidence report
        #[arg(long, conflicts_with_all = ["offset", "max_bytes"])]
        transfer_report: Option<std::path::PathBuf>,
    },
    /// Delete a file from a storage backend
    Delete {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        path: String,
    },
    /// Begin an upload without sending data (diagnostic use)
    Begin {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        size: u32,
        #[arg(long)]
        crc32: u32,
    },
    /// Upload a file to a storage backend
    Upload(UploadArgs),
    /// Abort an in-progress upload by its device-assigned ID
    Abort {
        #[arg(short, long)]
        port: String,
        #[arg(long)]
        upload_id: u32,
    },
}
