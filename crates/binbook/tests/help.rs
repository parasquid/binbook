use std::process::Command;

#[test]
fn help_names_binbook_and_exposes_locked_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_binbook"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("CLI tool for BinBook"));
    for command in ["encode", "decode", "inspect", "diag"] {
        assert!(text.contains(command), "missing {command}: {text}");
    }
    assert!(!text.contains(&["encode", "png", "folder"].join("-")));
}

#[test]
fn encode_help_documents_examples_and_locked_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_binbook"))
        .args(["encode", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for value in [
        "--input-format",
        "--profile",
        "--pixel-format",
        "--no-dither",
        "--font-family",
        "binbook encode",
    ] {
        assert!(text.contains(value), "missing {value}: {text}");
    }
}

#[test]
fn upload_help_exposes_progress_integrity_and_destination_options() {
    for args in [
        vec!["upload", "--help"],
        vec!["diag", "storage", "upload", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_binbook"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        for option in [
            "--port",
            "--file",
            "--name",
            "--path",
            "--progress",
            "--transfer-mode",
            "--window",
            "--no-wait-library",
            "--json",
            "--transfer-report",
        ] {
            assert!(text.contains(option), "missing {option}: {text}");
        }
    }
}

#[cfg(feature = "serial-device")]
#[test]
fn upload_transfer_mode_and_window_parse_explicit_bulk_and_legacy_controls() {
    use binbook::{Cli, Commands, RequestedTransferMode, RequestedTransferWindow};
    use clap::Parser;

    let cli = Cli::try_parse_from([
        "binbook",
        "upload",
        "--port",
        "fake",
        "--file",
        "book.binbook",
        "--transfer-mode",
        "bulk",
        "--window",
        "4",
    ])
    .unwrap();
    let Commands::Upload(args) = cli.command else {
        panic!("expected upload")
    };
    assert_eq!(args.transfer_mode, RequestedTransferMode::Bulk);
    assert_eq!(args.window, RequestedTransferWindow::Four);

    let cli = Cli::try_parse_from([
        "binbook",
        "upload",
        "--port",
        "fake",
        "--file",
        "book.binbook",
        "--transfer-mode",
        "legacy",
        "--window",
        "1",
    ])
    .unwrap();
    let Commands::Upload(args) = cli.command else {
        panic!("expected upload")
    };
    assert_eq!(args.transfer_mode, RequestedTransferMode::Legacy);
    assert_eq!(args.window, RequestedTransferWindow::One);
}

#[cfg(feature = "serial-device")]
#[test]
fn read_help_exposes_bulk_controls_and_range_reads_reject_transfer_only_flags() {
    use binbook::Cli;
    use clap::Parser;

    let output = Command::new(env!("CARGO_BIN_EXE_binbook"))
        .args(["diag", "storage", "read", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for option in [
        "--progress",
        "--transfer-mode",
        "--window",
        "--transfer-report",
    ] {
        assert!(text.contains(option), "missing {option}: {text}");
    }

    assert!(Cli::try_parse_from([
        "binbook",
        "diag",
        "storage",
        "read",
        "--port",
        "fake",
        "--path",
        "book.binbook",
        "--output",
        "out.binbook",
        "--offset",
        "1",
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "binbook",
        "diag",
        "storage",
        "read",
        "--port",
        "fake",
        "--path",
        "book.binbook",
        "--output",
        "out.binbook",
        "--offset",
        "1",
        "--transfer-mode",
        "bulk",
    ])
    .is_err());
    assert!(Cli::try_parse_from([
        "binbook",
        "diag",
        "storage",
        "read",
        "--port",
        "fake",
        "--path",
        "book.binbook",
        "--output",
        "out.binbook",
        "--offset",
        "1",
        "--transfer-report",
        "report.json",
    ])
    .is_err());
}

#[test]
#[cfg(feature = "serial-device")]
fn upload_destination_defaults_to_the_source_filename() {
    let output = Command::new(env!("CARGO_BIN_EXE_binbook"))
        .args([
            "upload",
            "--port",
            "/definitely/missing-port",
            "--file",
            "/definitely/missing-book.binbook",
            "--progress",
            "never",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("missing-book.binbook"), "{stderr}");
    assert!(
        !stderr.contains("required arguments were not provided"),
        "{stderr}"
    );
}
