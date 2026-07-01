use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::io;
use crate::CliError;

static NONCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_atomic<T>(
    destination: &Path,
    write: impl FnOnce(&mut File) -> Result<T, CliError>,
) -> Result<T, CliError> {
    let temporary = temporary_path(destination)?;
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| io(&temporary, source))?;
    let result = write(&mut file).and_then(|value| {
        file.sync_all().map_err(|source| io(&temporary, source))?;
        Ok(value)
    });
    drop(file);
    match result {
        Ok(value) => match fs::rename(&temporary, destination) {
            Ok(()) => Ok(value),
            Err(source) => {
                let _ = fs::remove_file(&temporary);
                Err(io(destination, source))
            }
        },
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn temporary_path(destination: &Path) -> Result<PathBuf, CliError> {
    let name = destination.file_name().ok_or(CliError::InvalidOutputPath)?;
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    Ok(destination.with_file_name(temporary))
}
