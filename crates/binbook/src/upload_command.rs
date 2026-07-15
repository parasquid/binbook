use std::fs::File;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::time::Duration;

use binbook_diagnostic_protocol::{decode_hello_response, Opcode, CAP_STORAGE_UPLOAD_WINDOW_2};

use crate::{
    progress::TextProgress,
    serial_transport::SerialSession,
    upload::{
        upload_stream, SystemClock, UploadError, UploadPacket, UploadRequest, UploadSummary,
        UploadTransport,
    },
    ProgressMode, UploadArgs,
};

const STORAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn run_command(args: &UploadArgs) -> Result<(), UploadError> {
    let destination = destination_name(&args.file, args.name.as_deref())?;
    let mut source = File::open(&args.file).map_err(|source| UploadError::SourceOpen {
        path: args.file.clone(),
        source,
    })?;
    let mut transport =
        SerialSession::open(&args.port).map_err(|message| UploadError::Transport {
            message,
            cleanup_confirmed: false,
        })?;
    let stderr = io::stderr();
    let is_tty = stderr.is_terminal();
    let mut stderr = stderr.lock();
    let mode = args.progress.unwrap_or(if args.json {
        ProgressMode::Never
    } else {
        ProgressMode::Auto
    });
    let mut progress = TextProgress::new(&mut stderr, mode, is_tty, destination);
    let summary = upload_stream(
        &mut source,
        UploadRequest::new(destination, args.no_wait_library),
        &mut transport,
        &mut progress,
        &SystemClock::start(),
    )?;
    progress.result().map_err(UploadError::Progress)?;
    print_summary(destination, &summary, args.json);
    Ok(())
}

fn destination_name<'a>(
    source: &'a Path,
    explicit: Option<&'a str>,
) -> Result<&'a str, UploadError> {
    explicit
        .or_else(|| source.file_name().and_then(|name| name.to_str()))
        .ok_or(UploadError::DestinationName)
}

fn print_summary(destination: &str, summary: &UploadSummary, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "bytes": summary.bytes,
                "bytes_per_second": summary.bytes_per_second,
                "crc32": format!("0x{:08X}", summary.crc32),
                "destination": destination,
                "elapsed_ms": summary.elapsed_ms,
                "library_visible": summary.library_visible,
            })
        );
    } else {
        println!(
            "Uploaded {destination} ({} bytes, CRC32=0x{:08X}, {} B/s, library_visible={})",
            summary.bytes, summary.crc32, summary.bytes_per_second, summary.library_visible
        );
    }
}

impl UploadTransport for SerialSession {
    fn upload_window(&mut self) -> Result<usize, UploadError> {
        const HELLO_SEQUENCE: u16 = u16::MAX;
        let request = crate::diag_protocol::hello_request(HELLO_SEQUENCE);
        let response = self
            .exchange_frame_batch(&request, &[HELLO_SEQUENCE], STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)?
            .into_iter()
            .next()
            .ok_or_else(|| UploadError::Response("HELLO response was absent".into()))?;
        let hello_payload =
            crate::upload_protocol::checked_payload(&response, Opcode::Hello, HELLO_SEQUENCE)?;
        let hello = decode_hello_response(&hello_payload)?;
        Ok(if hello.capabilities & CAP_STORAGE_UPLOAD_WINDOW_2 != 0 {
            2
        } else {
            1
        })
    }

    fn exchange(
        &mut self,
        request: &[u8],
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Vec<u8>, UploadError> {
        self.exchange_frame_batch(request, &[sequence], STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                UploadError::Response(format!(
                    "response was absent for {opcode:?} sequence {sequence}"
                ))
            })
    }

    fn exchange_batch(&mut self, requests: &[UploadPacket]) -> Result<Vec<Vec<u8>>, UploadError> {
        let bytes = requests
            .iter()
            .flat_map(|request| request.frame.iter().copied())
            .collect::<Vec<_>>();
        let sequences = requests
            .iter()
            .map(|request| request.sequence)
            .collect::<Vec<_>>();
        self.exchange_frame_batch(&bytes, &sequences, STORAGE_REQUEST_TIMEOUT)
            .map_err(transport_error)
    }

    fn wait_library_poll(&mut self) {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn transport_error(message: String) -> UploadError {
    UploadError::Transport {
        message,
        cleanup_confirmed: false,
    }
}
