pub mod protocol {
    pub fn list_command() -> String {
        "LIST\n".to_owned()
    }

    pub fn delete_command(name: &str) -> String {
        format!("DELETE {name}\n")
    }

    pub fn upload_command(name: &str, size: u64) -> String {
        format!("UPLOAD {name} {size}\n")
    }
}
