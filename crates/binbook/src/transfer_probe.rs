use binbook_diagnostic_protocol::{
    decode_hello_response, decode_transfer_limits, Opcode, TransferDirection,
    CAP_STORAGE_TRANSFER_LIMITS,
};

use crate::{
    transfer_negotiation::{negotiate_transfer, RequestedMode, RequestedWindow, TransferSelection},
    upload::{UploadError, UploadTransport},
    upload_protocol::{checked_payload, encode_request},
};

const HELLO_SEQUENCE: u16 = u16::MAX;
const LIMITS_SEQUENCE: u16 = u16::MAX - 1;

pub fn probe_transfer<T: UploadTransport>(
    transport: &mut T,
    direction: TransferDirection,
    requested_mode: RequestedMode,
    requested_window: RequestedWindow,
) -> Result<TransferSelection, UploadError> {
    let hello_frame = encode_request(HELLO_SEQUENCE, Opcode::Hello, |_| Ok(0))?;
    let hello_response = match transport.exchange(&hello_frame, Opcode::Hello, HELLO_SEQUENCE) {
        Ok(response) => response,
        Err(UploadError::Transport { .. }) => {
            transport.exchange(&hello_frame, Opcode::Hello, HELLO_SEQUENCE)?
        }
        Err(error) => return Err(error),
    };
    let hello_payload = checked_payload(&hello_response, Opcode::Hello, HELLO_SEQUENCE)?;
    let hello = decode_hello_response(&hello_payload)?;

    let limits = if hello.capabilities & CAP_STORAGE_TRANSFER_LIMITS == 0 {
        None
    } else {
        let limits_frame = encode_request(LIMITS_SEQUENCE, Opcode::StoreTransferLimits, |_| Ok(0))?;
        let limits_response =
            transport.exchange(&limits_frame, Opcode::StoreTransferLimits, LIMITS_SEQUENCE)?;
        let limits_payload = checked_payload(
            &limits_response,
            Opcode::StoreTransferLimits,
            LIMITS_SEQUENCE,
        )?;
        Some(decode_transfer_limits(&limits_payload)?)
    };

    negotiate_transfer(
        hello.capabilities,
        limits,
        direction,
        requested_mode,
        requested_window,
    )
    .map_err(|error| UploadError::Response(error.to_string()))
}
