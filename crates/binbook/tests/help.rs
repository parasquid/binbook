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
            "--no-wait-library",
            "--json",
        ] {
            assert!(text.contains(option), "missing {option}: {text}");
        }
    }
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
