use std::io::{self, Write};

use crate::{
    upload::{ProgressEvent, ProgressSink},
    ProgressMode,
};

pub struct TextProgress<'a, W> {
    writer: &'a mut W,
    mode: ProgressMode,
    is_tty: bool,
    name: &'a str,
    activity: &'static str,
    line_active: bool,
    last_render_ms: Option<u64>,
    error: Option<io::Error>,
}

impl<'a, W: Write> TextProgress<'a, W> {
    pub fn new(writer: &'a mut W, mode: ProgressMode, is_tty: bool, name: &'a str) -> Self {
        Self {
            writer,
            mode,
            is_tty,
            name,
            activity: "Uploading",
            line_active: false,
            last_render_ms: None,
            error: None,
        }
    }

    pub fn for_readback(
        writer: &'a mut W,
        mode: ProgressMode,
        is_tty: bool,
        name: &'a str,
    ) -> Self {
        let mut progress = Self::new(writer, mode, is_tty, name);
        progress.activity = "Reading";
        progress
    }

    pub fn result(&self) -> io::Result<()> {
        match &self.error {
            Some(error) => Err(io::Error::new(error.kind(), error.to_string())),
            None => Ok(()),
        }
    }

    fn render(&mut self, event: ProgressEvent) -> io::Result<()> {
        if self.mode == ProgressMode::Never {
            return Ok(());
        }
        match event {
            ProgressEvent::Preparing { total } => {
                self.finish_active_line()?;
                writeln!(
                    self.writer,
                    "Preparing storage for {} ({total} bytes)",
                    self.name
                )
            }
            ProgressEvent::Transfer {
                acknowledged,
                total,
                elapsed_ms,
            } => self.render_transfer(acknowledged, total, elapsed_ms),
            ProgressEvent::Verifying => {
                self.finish_active_line()?;
                writeln!(self.writer, "Verifying CRC32")
            }
            ProgressEvent::Publishing => {
                self.finish_active_line()?;
                writeln!(self.writer, "Publishing")
            }
            ProgressEvent::UpdatingLibrary => {
                self.finish_active_line()?;
                writeln!(self.writer, "Updating library")
            }
            ProgressEvent::Complete(_) => self.finish_active_line(),
        }
    }

    fn render_transfer(
        &mut self,
        acknowledged: u64,
        total: u64,
        elapsed_ms: u64,
    ) -> io::Result<()> {
        let final_update = acknowledged == total;
        let throttled = self
            .last_render_ms
            .is_some_and(|last| elapsed_ms.saturating_sub(last) < 100);
        if throttled && !final_update {
            return Ok(());
        }
        self.last_render_ms = Some(elapsed_ms);

        let percentage = if total == 0 || acknowledged >= total {
            100.0
        } else {
            acknowledged as f64 * 100.0 / total as f64
        };
        let rate = if elapsed_ms == 0 {
            0
        } else {
            (acknowledged as u128 * 1_000 / elapsed_ms as u128) as u64
        };
        let eta = if acknowledged == total {
            "0.0s".to_owned()
        } else if rate == 0 {
            "unknown".to_owned()
        } else {
            format!(
                "{:.1}s",
                total.saturating_sub(acknowledged) as f64 / rate as f64
            )
        };
        let prefix = if self.is_tty { "\r" } else { "" };
        let suffix = if self.is_tty { "" } else { "\n" };
        write!(
            self.writer,
            "{prefix}{} {acknowledged}/{total} bytes ({percentage:.1}%) at {rate} B/s; elapsed {:.1}s; ETA {eta}{suffix}",
            self.activity,
            elapsed_ms as f64 / 1_000.0
        )?;
        self.line_active = self.is_tty;
        Ok(())
    }

    fn finish_active_line(&mut self) -> io::Result<()> {
        if self.line_active {
            writeln!(self.writer)?;
            self.line_active = false;
        }
        Ok(())
    }
}

impl<W: Write> ProgressSink for TextProgress<'_, W> {
    fn report(&mut self, event: ProgressEvent) {
        if self.error.is_none() {
            if let Err(error) = self.render(event) {
                self.error = Some(error);
            }
        }
    }
}
