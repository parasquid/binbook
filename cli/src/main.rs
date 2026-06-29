use binbook_cli::{Cli, Commands, DiagCommand, ExerciseCommand, PageAction, ProbeCommand};
use clap::Parser;

#[cfg(feature = "serial-device")]
fn run_diag(cmd: DiagCommand) {
    use binbook_cli::diag_protocol;
    use binbook_cli::serial_transport::SerialSession;
    use binbook_diagnostic_protocol::{KeyCode, Opcode};
    use std::time::Duration;

    if let DiagCommand::Exercise { exercise } = &cmd {
        match exercise {
            ExerciseCommand::StagedGray { port } => {
                if let Err(error) = binbook_cli::exercise::run_staged_gray(port) {
                    eprintln!("Communication error: {error}");
                    std::process::exit(1);
                }
                return;
            }
        }
    }

    let (port, frame, expected_opcode, timeout) = match &cmd {
        DiagCommand::Hello { port } => (
            port.clone(),
            diag_protocol::hello_request(1),
            Opcode::Hello,
            Duration::from_secs(2),
        ),
        DiagCommand::Key { port, key } => {
            let kc = match key.as_str() {
                "LEFT" => KeyCode::Left,
                "RIGHT" => KeyCode::Right,
                "UP" => KeyCode::Up,
                "DOWN" => KeyCode::Down,
                "SELECT" => KeyCode::Select,
                "BACK" => KeyCode::Back,
                "POWER" => KeyCode::Power,
                _ => unreachable!(),
            };
            (
                port.clone(),
                diag_protocol::key_request(1, kc),
                Opcode::Key,
                Duration::from_secs(30),
            )
        }
        DiagCommand::Page { port, action } => {
            let frame = match action {
                PageAction::Next => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Next,
                ),
                PageAction::Previous => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Previous,
                ),
                PageAction::First => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::First,
                ),
                PageAction::Last => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Last,
                ),
                PageAction::Current => diag_protocol::page_action_request(
                    1,
                    binbook_diagnostic_protocol::PageAction::Current,
                ),
                PageAction::Goto { page } => diag_protocol::page_goto_request(1, *page),
            };
            (port.clone(), frame, Opcode::Page, Duration::from_secs(30))
        }
        DiagCommand::Status { port } => (
            port.clone(),
            diag_protocol::status_request(1),
            Opcode::Status,
            Duration::from_secs(2),
        ),
        DiagCommand::Logs { port, since, clear } => {
            if *clear {
                (
                    port.clone(),
                    diag_protocol::log_clear_request(1),
                    Opcode::LogClear,
                    Duration::from_secs(2),
                )
            } else {
                let cursor = since.unwrap_or(0);
                (
                    port.clone(),
                    diag_protocol::log_get_request(1, cursor, 496),
                    Opcode::LogGet,
                    Duration::from_secs(2),
                )
            }
        }
        DiagCommand::Crash { port, clear } => {
            if *clear {
                (
                    port.clone(),
                    diag_protocol::crash_clear_request(1),
                    Opcode::CrashClear,
                    Duration::from_secs(2),
                )
            } else {
                (
                    port.clone(),
                    diag_protocol::crash_get_request(1),
                    Opcode::CrashGet,
                    Duration::from_secs(2),
                )
            }
        }
        DiagCommand::Probe { port, probe } => {
            let choice = match probe {
                ProbeCommand::WindowCorners => diag_protocol::ProbeChoice::WindowCorners,
                ProbeCommand::ClearWhite => diag_protocol::ProbeChoice::ClearWhite,
                ProbeCommand::FullRefreshCurrent => diag_protocol::ProbeChoice::FullRefreshCurrent,
            };
            (
                port.clone(),
                diag_protocol::display_probe_request(1, choice),
                Opcode::DisplayProbe,
                binbook_cli::DISPLAY_PROBE_TIMEOUT,
            )
        }
        DiagCommand::Exercise { .. } => unreachable!("exercise handled above"),
    };

    let mut session = match SerialSession::open(&port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    match session.send_and_receive(&frame, expected_opcode, 1, timeout) {
        Ok(response) => match diag_protocol::format_response(&response, expected_opcode, 1) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                eprintln!("Response error: {error}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Communication error: {e}");
            std::process::exit(1);
        }
    }
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
        Commands::Flash { port, firmware } => {
            println!("Flashing {} to {}...", firmware.display(), port);
        }
        Commands::Upload { port, file, name } => {
            println!("Uploading {} as {} to {}...", file.display(), name, port);
        }
        Commands::List { port } => {
            println!("Listing books on {}...", port);
        }
        Commands::Delete { port, name } => {
            println!("Deleting {} from {}...", name, port);
        }
        Commands::Diag(cmd) => run_diag(cmd),
    }
}
