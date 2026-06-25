use binbook_cli::protocol::{delete_command, list_command, upload_command};

#[test]
fn formats_serial_protocol_commands() {
    assert_eq!(list_command(), "LIST\n");
    assert_eq!(delete_command("sample.binbook"), "DELETE sample.binbook\n");
    assert_eq!(
        upload_command("sample.binbook", 12345),
        "UPLOAD sample.binbook 12345\n",
    );
}
