//! BinBook diagnostic serial protocol for Xteink X4 firmware.
//!
//! This crate provides COBS-framed binary packet encoding/decoding for the
//! diagnostic serial console. It is designed for no_std firmware use with
//! caller-owned buffers and no heap allocation.

#![no_std]

mod bulk_control;
mod bulk_data;
mod bulk_open;
mod cobs;
mod events;
mod frame;
mod key_page;
mod probe;
mod refresh_mode;
mod sd_write_trace;
mod status_log;
mod storage;
mod transfer_limits;
mod transfer_trace;
mod types;

pub use bulk_control::*;
pub use bulk_data::*;
pub use bulk_open::*;
pub use cobs::{cobs_decode, cobs_decode_in_place, cobs_encode, crc16_ccitt_false};
pub use events::*;
pub use frame::{
    decode_frame, decode_frame_in_place, decode_frame_in_place_profiled, decode_frame_profiled,
    decode_raw_frame, encode_frame, encode_raw_frame, FrameDecodeEvent,
};
pub use key_page::{
    decode_hello_response, decode_key_payload, decode_page_payload, encode_hello_response,
    encode_key_payload, encode_page_payload, encode_page_response,
};
pub use probe::{decode_probe_payload, encode_probe_payload};
pub use refresh_mode::{decode_refresh_mode_payload, encode_refresh_mode_payload};
pub use sd_write_trace::*;
pub use status_log::{
    decode_crash_response, decode_log_get_payload, decode_log_record, decode_log_response_header,
    decode_status_payload, encode_crash_response, encode_log_get_payload, encode_log_record,
    encode_log_response_header, encode_status_payload, LOG_RECORD_BYTES,
};
pub use storage::*;
pub use transfer_limits::*;
pub use transfer_trace::*;
pub use types::*;
