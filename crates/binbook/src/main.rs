use binbook::{Cli, Commands, DiagCommand, StorageCommand};
use clap::Parser;

#[cfg(feature = "serial-device")]
fn run_diag(cmd: DiagCommand) {
    binbook::diag_cli::run_diag(cmd);
}

#[cfg(feature = "serial-device")]
fn run_upload(args: binbook::UploadArgs) {
    if let Err(error) = binbook::upload_command::run_command(&args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "serial-device"))]
fn run_upload(args: binbook::UploadArgs) {
    let _ = args;
    eprintln!("serial-device feature is not enabled. Rebuild with --features serial-device.");
    std::process::exit(1);
}

#[cfg(not(feature = "serial-device"))]
fn run_diag(cmd: DiagCommand) {
    let _ = cmd;
    eprintln!("serial-device feature is not enabled. Rebuild with --features serial-device.");
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            input,
            output,
            input_format,
            profile,
            pixel_format,
            no_dither,
            font_family,
        } => exit_on_error(binbook::run_encode(
            &input,
            &output,
            input_format,
            profile,
            pixel_format,
            no_dither,
            font_family,
        )),
        Commands::Decode { book, page, output } => {
            exit_on_error(binbook::run_decode(&book, page, &output));
        }
        Commands::Inspect {
            book,
            validate,
            strict,
            json,
        } => exit_on_error(binbook::run_inspect(&book, validate, strict, json)),
        Commands::Flash { port, firmware } => {
            println!("Flashing {} to {}...", firmware.display(), port);
        }
        Commands::Upload(args) => run_upload(args),
        Commands::List { port } => run_diag(DiagCommand::Storage(StorageCommand::List {
            port,
            path: None,
        })),
        Commands::Delete { port, name } => {
            run_diag(DiagCommand::Storage(StorageCommand::Delete {
                port,
                path: name,
            }));
        }
        Commands::Diag(cmd) => run_diag(cmd),
    }
}

fn exit_on_error(result: Result<(), binbook::CliError>) {
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
