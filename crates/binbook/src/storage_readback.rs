use std::io::Write;
use std::time::Duration;

use binbook_diagnostic_protocol::{
    decode_hello_response, decode_store_read_response, Opcode, CAP_STORAGE_READ_WINDOW_2,
    MAX_PAYLOAD_BYTES,
};
use thiserror::Error;

use crate::{
    diag_protocol, serial_transport::SerialSession, upload_protocol::checked_payload,
    upload_types::UploadError,
};

const READ_BYTES: u32 = (MAX_PAYLOAD_BYTES - 5) as u32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const HELLO_SEQUENCE: u16 = u16::MAX;

pub trait ReadTransport {
    fn read_window(&mut self) -> Result<usize, ReadbackError>;
    fn exchange_batch(
        &mut self,
        requests: &[Vec<u8>],
        sequences: &[u16],
    ) -> Result<Vec<Vec<u8>>, ReadbackError>;
}

#[derive(Debug, Error)]
pub enum ReadbackError {
    #[error("transport failed: {0}")]
    Transport(String),
    #[error("protocol failed: {0}")]
    Protocol(String),
    #[error("output failed: {0}")]
    Output(#[from] std::io::Error),
    #[error("read returned no data before EOF at offset {0}")]
    Empty(u32),
}

pub fn read_stream<T: ReadTransport, W: Write>(
    path: &str,
    transport: &mut T,
    output: &mut W,
) -> Result<u64, ReadbackError> {
    let window = transport.read_window()?.clamp(1, 2);
    let mut offset = 0u32;
    let mut sequence = 1u16;
    loop {
        let mut requests = Vec::with_capacity(window);
        let mut sequences = Vec::with_capacity(window);
        for slot in 0..window {
            let request_offset = offset
                .checked_add(READ_BYTES.saturating_mul(slot as u32))
                .ok_or_else(|| ReadbackError::Protocol("read offset overflow".into()))?;
            requests.push(diag_protocol::store_read_request_range(
                sequence,
                path,
                request_offset,
                READ_BYTES,
            ));
            sequences.push(sequence);
            sequence = sequence.wrapping_add(1).max(1);
        }
        let responses = transport.exchange_batch(&requests, &sequences)?;
        if responses.len() != requests.len() {
            return Err(ReadbackError::Protocol(format!(
                "expected {} responses, got {}",
                requests.len(),
                responses.len()
            )));
        }
        for (response, expected_sequence) in responses.iter().zip(sequences) {
            let payload = checked_payload(response, Opcode::StoreRead, expected_sequence)
                .map_err(protocol_error)?;
            let read = decode_store_read_response(&payload).map_err(|error| {
                ReadbackError::Protocol(format!("invalid read payload: {error:?}"))
            })?;
            if read.data.is_empty() && !read.eof {
                return Err(ReadbackError::Empty(offset));
            }
            output.write_all(read.data)?;
            offset = offset
                .checked_add(read.data.len() as u32)
                .ok_or_else(|| ReadbackError::Protocol("read size overflow".into()))?;
            if read.eof {
                output.flush()?;
                return Ok(u64::from(offset));
            }
        }
    }
}

pub fn read_file(path: &str, port: &str, output: &mut impl Write) -> Result<u64, ReadbackError> {
    let mut session = SerialSession::open(port).map_err(ReadbackError::Transport)?;
    read_stream(path, &mut session, output)
}

impl ReadTransport for SerialSession {
    fn read_window(&mut self) -> Result<usize, ReadbackError> {
        let request = diag_protocol::hello_request(HELLO_SEQUENCE);
        let response = self
            .exchange_frame_batch(&request, &[HELLO_SEQUENCE], REQUEST_TIMEOUT)
            .map_err(ReadbackError::Transport)?
            .into_iter()
            .next()
            .ok_or_else(|| ReadbackError::Protocol("HELLO response was absent".into()))?;
        let payload =
            checked_payload(&response, Opcode::Hello, HELLO_SEQUENCE).map_err(protocol_error)?;
        let hello = decode_hello_response(&payload)
            .map_err(|error| ReadbackError::Protocol(format!("invalid HELLO: {error:?}")))?;
        Ok(if hello.capabilities & CAP_STORAGE_READ_WINDOW_2 != 0 {
            2
        } else {
            1
        })
    }

    fn exchange_batch(
        &mut self,
        requests: &[Vec<u8>],
        sequences: &[u16],
    ) -> Result<Vec<Vec<u8>>, ReadbackError> {
        let bytes = requests
            .iter()
            .flat_map(|request| request.iter().copied())
            .collect::<Vec<_>>();
        self.exchange_frame_batch(&bytes, sequences, REQUEST_TIMEOUT)
            .map_err(ReadbackError::Transport)
    }
}

fn protocol_error(error: UploadError) -> ReadbackError {
    ReadbackError::Protocol(error.to_string())
}
