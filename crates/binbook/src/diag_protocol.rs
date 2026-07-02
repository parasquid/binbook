use binbook_diagnostic_protocol::{
    encode_frame, encode_log_get_payload, encode_page_payload, encode_probe_payload,
    encode_store_delete_request, encode_store_list_request, encode_store_read_request,
    FrameHeader, FrameKind, KeyAction, KeyCode, LogGetPayload, Opcode, PageAction, ProbeCode,
    Status, StorageBackend, StoreDeleteRequest, StoreListRequest, StoreReadRequest,
    MAX_FRAME_BYTES,
};

pub use crate::diag_response::{
    decode_hello_response_payload, decode_status_response, format_response, StatusResponse,
};

fn request_header(sequence: u16, opcode: Opcode) -> FrameHeader {
    FrameHeader {
        kind: FrameKind::Request,
        opcode,
        status: Status::Ok,
        sequence,
        payload_len: 0,
    }
}

pub fn hello_request(sequence: u16) -> Vec<u8> {
    let header = request_header(sequence, Opcode::Hello);
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn key_request(sequence: u16, key: KeyCode) -> Vec<u8> {
    let mut header = request_header(sequence, Opcode::Key);
    header.payload_len = 2;
    let payload = [key as u8, KeyAction::Press as u8];
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn page_goto_request(sequence: u16, page: u32) -> Vec<u8> {
    let mut header = request_header(sequence, Opcode::Page);
    let mut payload_buf = [0u8; 5];
    let plen = encode_page_payload(PageAction::Goto, Some(page), &mut payload_buf).unwrap();
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn page_action_request(sequence: u16, action: PageAction) -> Vec<u8> {
    let mut header = request_header(sequence, Opcode::Page);
    let mut payload_buf = [0u8; 5];
    let plen = encode_page_payload(action, None, &mut payload_buf).unwrap();
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn status_request(sequence: u16) -> Vec<u8> {
    let header = request_header(sequence, Opcode::Status);
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn log_get_request(sequence: u16, cursor: u32, max_bytes: u16) -> Vec<u8> {
    let mut header = request_header(sequence, Opcode::LogGet);
    let mut payload_buf = [0u8; 6];
    let plen = encode_log_get_payload(
        LogGetPayload {
            cursor_sequence: cursor,
            max_bytes,
        },
        &mut payload_buf,
    )
    .unwrap();
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn log_clear_request(sequence: u16) -> Vec<u8> {
    let header = request_header(sequence, Opcode::LogClear);
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn crash_get_request(sequence: u16) -> Vec<u8> {
    let header = request_header(sequence, Opcode::CrashGet);
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn crash_clear_request(sequence: u16) -> Vec<u8> {
    let header = request_header(sequence, Opcode::CrashClear);
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    buf[..len].to_vec()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeChoice {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
}

impl ProbeChoice {
    pub fn to_probe_code(self) -> ProbeCode {
        match self {
            Self::FullRefreshCurrent => ProbeCode::FullRefreshCurrent,
            Self::ClearWhite => ProbeCode::ClearWhite,
            Self::WindowCorners => ProbeCode::WindowCorners,
        }
    }
}

pub fn store_list_request(sequence: u16, path: &str) -> Vec<u8> {
    let payload = StoreListRequest {
        backend: StorageBackend::Sd,
        path,
    };
    let mut payload_buf = [0u8; 512];
    let plen = encode_store_list_request(&payload, &mut payload_buf).unwrap();
    let mut header = request_header(sequence, Opcode::StoreList);
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn store_read_request(sequence: u16, path: &str) -> Vec<u8> {
    let payload = StoreReadRequest {
        backend: StorageBackend::Sd,
        path,
    };
    let mut payload_buf = [0u8; 512];
    let plen = encode_store_read_request(&payload, &mut payload_buf).unwrap();
    let mut header = request_header(sequence, Opcode::StoreRead);
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn store_delete_request(sequence: u16, path: &str) -> Vec<u8> {
    let payload = StoreDeleteRequest {
        backend: StorageBackend::Sd,
        path,
    };
    let mut payload_buf = [0u8; 512];
    let plen = encode_store_delete_request(&payload, &mut payload_buf).unwrap();
    let mut header = request_header(sequence, Opcode::StoreDelete);
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}

pub fn display_probe_request(sequence: u16, probe: ProbeChoice) -> Vec<u8> {
    let mut header = request_header(sequence, Opcode::DisplayProbe);
    let mut payload_buf = [0u8; 1];
    let plen = encode_probe_payload(probe.to_probe_code(), &mut payload_buf).unwrap();
    header.payload_len = plen as u16;
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload_buf[..plen], &mut buf).unwrap();
    buf[..len].to_vec()
}
