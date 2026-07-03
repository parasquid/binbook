//! BinBook diagnostic serial protocol for Xteink X4 firmware.
//!
//! This crate provides COBS-framed binary packet encoding/decoding for the
//! diagnostic serial console. It is designed for no_std firmware use with
//! caller-owned buffers and no heap allocation.

#![no_std]

mod cobs;
mod events;
mod frame;
mod key_page;
mod probe;
mod refresh_mode;
mod status_log;
mod storage;
mod types;

pub use cobs::{cobs_decode, cobs_encode, crc16_ccitt_false};
pub use events::*;
pub use frame::{decode_frame, decode_raw_frame, encode_frame, encode_raw_frame};
pub use key_page::{
    decode_hello_response, decode_key_payload, decode_page_payload, encode_hello_response,
    encode_key_payload, encode_page_payload, encode_page_response,
};
pub use probe::{decode_probe_payload, encode_probe_payload};
pub use refresh_mode::{decode_refresh_mode_payload, encode_refresh_mode_payload};
pub use status_log::{
    decode_crash_response, decode_log_get_payload, decode_log_record, decode_log_response_header,
    decode_status_payload, encode_crash_response, encode_log_get_payload, encode_log_record,
    encode_log_response_header, encode_status_payload, LOG_RECORD_BYTES,
};
pub use storage::*;
pub use types::*;
