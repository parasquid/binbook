use binbook_diagnostic_protocol::{
    decode_frame, decode_key_payload, decode_log_get_payload, decode_page_payload,
    decode_probe_payload, decode_raw_frame, encode_frame, encode_hello_response, encode_log_record,
    encode_log_response_header, encode_page_response, encode_raw_frame, encode_status_payload,
    FrameHeader, FrameKind, HelloResponse, KeyAction, KeyCode, LogRecordPayload, LogResponseHeader,
    Opcode, PageAction, PanelModeCode, ProbeCode, RawFrameHeader, Status, StatusPayload,
    ALL_CAPABILITIES, FRAME_DELIMITER, LOG_RECORD_BYTES, LOG_RESPONSE_HEADER_BYTES,
    MAX_FRAME_BYTES, MAX_PAYLOAD_BYTES, PROTOCOL_VERSION,
};

use crate::diag_log::{DiagLog, DiagLogRecord};
use crate::input::{apply_page_turn, Button, PageTurn};

const SERIAL_RX_BUF_SIZE: usize = MAX_FRAME_BYTES * 2;
const SERIAL_TX_BUF_SIZE: usize = MAX_FRAME_BYTES;
const FIRMWARE_NAME: &str = "binbook-fw";
const TARGET: &str = "xteink-x4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayProbeKind {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticSnapshot {
    pub current_page: u32,
    pub page_count: u32,
    pub panel_mode: PanelModeCode,
    pub dropped_log_count: u32,
    pub protocol_error_count: u32,
    pub last_error: i32,
}

impl DiagnosticSnapshot {
    pub fn status_payload(&self) -> StatusPayload {
        StatusPayload {
            current_page: self.current_page,
            page_count: self.page_count,
            panel_mode: self.panel_mode,
            dropped_log_count: self.dropped_log_count,
            protocol_error_count: self.protocol_error_count,
            last_error: self.last_error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchResult {
    RenderTurn { turn: PageTurn },
    RenderPage { target_page: u32 },
    NoAction,
    LogGet { cursor: u32, max_bytes: u16 },
    LogClear,
    CrashGet,
    CrashClear,
    DisplayProbe(DisplayProbeKind),
    Response { status: Status, payload_len: usize },
}

pub struct DiagnosticState {
    pub current_page: u32,
    pub page_count: u32,
    pub panel_mode: PanelModeCode,
    pub last_error: i32,
}

pub struct DiagnosticPendingQueue<const N: usize> {
    items: [Option<PendingCommand>; N],
    head: usize,
    len: usize,
}

impl<const N: usize> DiagnosticPendingQueue<N> {
    pub const fn new() -> Self {
        Self {
            items: [None; N],
            head: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn front(&self) -> Option<PendingCommand> {
        if self.len == 0 {
            None
        } else {
            self.items[self.head]
        }
    }

    pub fn try_push(&mut self, pending: PendingCommand) -> Result<(), PendingCommand> {
        if self.len == N {
            return Err(pending);
        }

        let index = (self.head + self.len) % N;
        self.items[index] = Some(pending);
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<PendingCommand> {
        if self.len == 0 {
            return None;
        }

        let pending = self.items[self.head];
        self.items[self.head] = None;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        pending
    }
}

pub struct DiagnosticLoopState<const PENDING: usize, const LOG: usize> {
    snapshot: DiagnosticSnapshot,
    pending: DiagnosticPendingQueue<PENDING>,
    log: DiagLog<LOG>,
}

impl<const PENDING: usize, const LOG: usize> DiagnosticLoopState<PENDING, LOG> {
    pub fn new(snapshot: DiagnosticSnapshot, log: DiagLog<LOG>) -> Self {
        Self {
            snapshot,
            pending: DiagnosticPendingQueue::new(),
            log,
        }
    }

    pub fn enqueue_pending(&mut self, pending: PendingCommand) -> Result<(), PendingCommand> {
        self.pending.try_push(pending)
    }

    pub fn enqueue_pending_with_status(&mut self, pending: PendingCommand) -> Status {
        match self.pending.try_push(pending) {
            Ok(()) => Status::Ok,
            Err(_) => Status::Error,
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn status_payload(&self) -> StatusPayload {
        self.snapshot.status_payload()
    }

    pub fn snapshot(&self) -> DiagnosticSnapshot {
        self.snapshot
    }

    pub fn update_snapshot(&mut self, snapshot: DiagnosticSnapshot) {
        self.snapshot = snapshot;
    }

    pub fn log_mut(&mut self) -> &mut DiagLog<LOG> {
        &mut self.log
    }

    pub fn resolve_log_get(&self, cursor: u32, max_bytes: u16, resp_buf: &mut [u8]) -> usize {
        resolve_log_get(&self.log, cursor, max_bytes, resp_buf)
    }

    pub fn complete_pending(&mut self) -> Option<PendingCommand> {
        self.pending.pop()
    }
}

pub struct CommandContext {
    pub current_page: u32,
    pub page_count: u32,
    pub last_error: i32,
    pub panel_mode: PanelModeCode,
    pub protocol_errors: u32,
    pub dropped_records: u32,
    pub tick_ms: u32,
}

impl CommandContext {
    pub fn new(current_page: u32, page_count: u32, last_error: i32, panel_mode: u8) -> Self {
        Self {
            current_page,
            page_count,
            last_error,
            panel_mode: PanelModeCode::from_u8(panel_mode).unwrap_or(PanelModeCode::Unknown),
            protocol_errors: 0,
            dropped_records: 0,
            tick_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    BufferFull,
}

pub struct FeedResult {
    pub consumed: usize,
    pub frame_ready: bool,
}

pub struct SerialState {
    rx_buf: [u8; SERIAL_RX_BUF_SIZE],
    tx_buf: [u8; SERIAL_TX_BUF_SIZE],
    rx_pos: usize,
    tx_pos: usize,
    protocol_errors: u32,
    discard_until_delimiter: bool,
}

impl SerialState {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; SERIAL_RX_BUF_SIZE],
            tx_buf: [0u8; SERIAL_TX_BUF_SIZE],
            rx_pos: 0,
            tx_pos: 0,
            protocol_errors: 0,
            discard_until_delimiter: false,
        }
    }

    pub fn feed_rx(&mut self, bytes: &[u8]) -> FeedResult {
        let mut consumed = 0;
        let mut frame_ready = false;

        for &byte in bytes {
            if self.discard_until_delimiter {
                consumed += 1;
                if byte == FRAME_DELIMITER {
                    self.discard_until_delimiter = false;
                }
                continue;
            }

            if self.rx_pos >= SERIAL_RX_BUF_SIZE {
                self.protocol_errors += 1;
                self.discard_until_delimiter = true;
                self.rx_pos = 0;
                consumed += 1;
                continue;
            }

            self.rx_buf[self.rx_pos] = byte;
            self.rx_pos += 1;
            consumed += 1;

            if byte == FRAME_DELIMITER && !frame_ready {
                frame_ready = true;
            }
        }

        FeedResult {
            consumed,
            frame_ready,
        }
    }

    pub fn next_frame(&mut self, out: &mut [u8; MAX_FRAME_BYTES]) -> Option<usize> {
        if self.rx_pos == 0 {
            return None;
        }

        let delimiter_pos = self.rx_buf[..self.rx_pos]
            .iter()
            .position(|&b| b == FRAME_DELIMITER)?;

        let frame_len = delimiter_pos + 1;

        if frame_len > MAX_FRAME_BYTES {
            self.protocol_errors += 1;
            self.rx_buf.copy_within(frame_len..self.rx_pos, 0);
            self.rx_pos -= frame_len;
            return self.next_frame(out);
        }

        out[..frame_len].copy_from_slice(&self.rx_buf[..frame_len]);
        self.rx_buf.copy_within(frame_len..self.rx_pos, 0);
        self.rx_pos -= frame_len;

        Some(frame_len)
    }

    pub fn queue_tx(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.tx_pos + frame.len() > SERIAL_TX_BUF_SIZE {
            return Err(TransportError::BufferFull);
        }
        self.tx_buf[self.tx_pos..self.tx_pos + frame.len()].copy_from_slice(frame);
        self.tx_pos += frame.len();
        Ok(())
    }

    pub fn pending_tx(&self) -> &[u8] {
        &self.tx_buf[..self.tx_pos]
    }

    pub fn consume_tx(&mut self, written: usize) {
        if written >= self.tx_pos {
            self.tx_pos = 0;
        } else {
            let remaining = self.tx_pos - written;
            self.tx_buf.copy_within(written..self.tx_pos, 0);
            self.tx_pos = remaining;
        }
    }

    pub fn protocol_error_count(&self) -> u32 {
        self.protocol_errors
    }
}

pub fn keycode_to_button(key: KeyCode) -> Option<Button> {
    match key {
        KeyCode::Left => Some(Button::Left),
        KeyCode::Right => Some(Button::Right),
        KeyCode::Up => Some(Button::Up),
        KeyCode::Down => Some(Button::Down),
        KeyCode::Select => Some(Button::Select),
        KeyCode::Back => Some(Button::Back),
        KeyCode::Power => Some(Button::Power),
    }
}

fn resolve_page_action(action: PageAction, page_index: Option<u32>, ctx: &CommandContext) -> u32 {
    match action {
        PageAction::Next => apply_page_turn(ctx.current_page, ctx.page_count, PageTurn::Next),
        PageAction::Previous => {
            apply_page_turn(ctx.current_page, ctx.page_count, PageTurn::Previous)
        }
        PageAction::First => 0,
        PageAction::Last => ctx.page_count.saturating_sub(1),
        PageAction::Goto => {
            let raw = page_index.unwrap_or(0);
            raw.min(ctx.page_count.saturating_sub(1))
        }
        PageAction::Current => ctx.current_page,
    }
}

pub fn dispatch_command(
    header: FrameHeader,
    payload: &[u8],
    ctx: &mut CommandContext,
    resp_buf: &mut [u8],
) -> DispatchResult {
    if header.kind != FrameKind::Request || header.status != Status::Ok {
        return DispatchResult::Response {
            status: Status::BadRequest,
            payload_len: 0,
        };
    }

    match header.opcode {
        Opcode::Hello => {
            let hello = HelloResponse {
                protocol_version: PROTOCOL_VERSION,
                max_frame_bytes: MAX_FRAME_BYTES as u16,
                capabilities: ALL_CAPABILITIES,
                firmware_name: FIRMWARE_NAME,
                target: TARGET,
            };
            match encode_hello_response(&hello, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::Key => {
            let key_payload = match decode_key_payload(payload) {
                Ok(value) => value,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            if key_payload.action != KeyAction::Press {
                return DispatchResult::NoAction;
            }
            let button = match keycode_to_button(key_payload.key) {
                Some(b) => b,
                None => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            let turn = crate::input::target_page_for_button(button);
            if apply_page_turn(ctx.current_page, ctx.page_count, turn) == ctx.current_page {
                DispatchResult::NoAction
            } else {
                DispatchResult::RenderTurn { turn }
            }
        }
        Opcode::Page => {
            let page_payload = match decode_page_payload(payload) {
                Ok(value) => value,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            if page_payload.action == PageAction::Goto {
                let raw = page_payload.page_index.unwrap_or(0);
                if raw >= ctx.page_count {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    };
                }
            }
            let target = resolve_page_action(page_payload.action, page_payload.page_index, ctx);
            if target == ctx.current_page {
                DispatchResult::NoAction
            } else {
                DispatchResult::RenderPage {
                    target_page: target,
                }
            }
        }
        Opcode::Status => {
            let status_payload = StatusPayload {
                current_page: ctx.current_page,
                page_count: ctx.page_count,
                panel_mode: ctx.panel_mode,
                dropped_log_count: ctx.dropped_records,
                protocol_error_count: ctx.protocol_errors,
                last_error: ctx.last_error,
            };
            match encode_status_payload(status_payload, resp_buf) {
                Ok(len) => DispatchResult::Response {
                    status: Status::Ok,
                    payload_len: len,
                },
                Err(_) => DispatchResult::Response {
                    status: Status::InternalError,
                    payload_len: 0,
                },
            }
        }
        Opcode::DisplayProbe => match decode_probe_payload(payload) {
            Ok(ProbeCode::FullRefreshCurrent) => {
                DispatchResult::DisplayProbe(DisplayProbeKind::FullRefreshCurrent)
            }
            Ok(ProbeCode::ClearWhite) => DispatchResult::DisplayProbe(DisplayProbeKind::ClearWhite),
            Ok(ProbeCode::WindowCorners) => {
                DispatchResult::DisplayProbe(DisplayProbeKind::WindowCorners)
            }
            Err(_) => DispatchResult::Response {
                status: Status::BadRequest,
                payload_len: 0,
            },
        },
        Opcode::LogGet => {
            let get_payload = match decode_log_get_payload(payload) {
                Ok(p) => p,
                Err(_) => {
                    return DispatchResult::Response {
                        status: Status::BadRequest,
                        payload_len: 0,
                    }
                }
            };
            DispatchResult::LogGet {
                cursor: get_payload.cursor_sequence,
                max_bytes: get_payload.max_bytes,
            }
        }
        Opcode::LogClear => DispatchResult::LogClear,
        Opcode::CrashGet => DispatchResult::CrashGet,
        Opcode::CrashClear => DispatchResult::CrashClear,
    }
}

/// Wire format: `[next_cursor: u32 LE, dropped_log_count: u32 LE, records...]`
/// Each record is `LOG_RECORD_BYTES` (24) bytes. Only whole records within `max_bytes` are included.
pub fn resolve_log_get<const N: usize>(
    log: &DiagLog<N>,
    cursor: u32,
    max_bytes: u16,
    resp_buf: &mut [u8],
) -> usize {
    let budget = (max_bytes as usize).min(resp_buf.len());
    if budget < LOG_RESPONSE_HEADER_BYTES {
        return 0;
    }
    let record_budget = budget - LOG_RESPONSE_HEADER_BYTES;
    let max_records = record_budget / LOG_RECORD_BYTES;
    if max_records == 0 {
        let header = LogResponseHeader {
            next_cursor: cursor.max(log.oldest_sequence().unwrap_or(log.next_sequence())),
            dropped_log_count: log.dropped_records(),
            record_count: 0,
        };
        return encode_log_response_header(header, resp_buf).unwrap_or(0);
    }

    let mut records = [DiagLogRecord::default(); 64];
    let read_limit = max_records.min(64);
    let result = log.read_from_sequence(cursor, &mut records[..read_limit]);

    let mut pos = LOG_RESPONSE_HEADER_BYTES;

    for i in 0..result.record_count as usize {
        if pos + LOG_RECORD_BYTES > budget {
            break;
        }
        let rec = LogRecordPayload {
            sequence: records[i].sequence,
            tick_ms: records[i].tick_ms,
            level: records[i].level,
            subsystem: records[i].subsystem,
            event: records[i].event,
            arg0: records[i].arg0,
            arg1: records[i].arg1,
            arg2: records[i].arg2,
        };
        if let Ok(n) = encode_log_record(rec, &mut resp_buf[pos..pos + LOG_RECORD_BYTES]) {
            pos += n;
        }
    }

    let record_count = ((pos - LOG_RESPONSE_HEADER_BYTES) / LOG_RECORD_BYTES) as u16;
    let _ = encode_log_response_header(
        LogResponseHeader {
            next_cursor: result.next_cursor,
            dropped_log_count: result.dropped_log_count,
            record_count,
        },
        resp_buf,
    );

    pos
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    RenderTurn { turn: PageTurn },
    RenderPage { target_page: u32 },
    DisplayProbe(DisplayProbeKind),
    CrashGet,
    CrashClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingCommand {
    pub header: FrameHeader,
    pub action: PendingAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommand {
    Hardware(PendingCommand),
    LogGet {
        header: FrameHeader,
        cursor: u32,
        max_bytes: u16,
    },
    LogClear {
        header: FrameHeader,
    },
    Immediate {
        header: FrameHeader,
        status: Status,
        payload: [u8; MAX_PAYLOAD_BYTES],
        payload_len: usize,
    },
}

impl RuntimeCommand {
    pub fn header(&self) -> FrameHeader {
        match *self {
            RuntimeCommand::Hardware(pending) => pending.header,
            RuntimeCommand::LogGet { header, .. }
            | RuntimeCommand::LogClear { header }
            | RuntimeCommand::Immediate { header, .. } => header,
        }
    }
}

/// Parse one request without reading or mutating diagnostic snapshot/log state.
/// The runtime aggregator supplies the committed snapshot and services log queries.
pub fn poll_runtime_command(
    serial: &mut SerialState,
    snapshot: DiagnosticSnapshot,
) -> Option<RuntimeCommand> {
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let frame_len = serial.next_frame(&mut frame_buf)?;
    let mut request_payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, payload_len) = match decode_frame(&frame_buf[..frame_len], &mut request_payload) {
        Ok(value) => value,
        Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode) => {
            if let Ok((raw, _)) = decode_raw_frame(&frame_buf[..frame_len], &mut request_payload) {
                if raw.kind == FrameKind::Request as u8 {
                    let response = RawFrameHeader {
                        kind: FrameKind::Response as u8,
                        opcode: raw.opcode,
                        status: Status::BadRequest as u8,
                        sequence: raw.sequence,
                        payload_len: 0,
                    };
                    let mut encoded = [0u8; MAX_FRAME_BYTES];
                    if let Ok(len) = encode_raw_frame(&response, &[], &mut encoded) {
                        let _ = serial.queue_tx(&encoded[..len]);
                    }
                }
            }
            serial.protocol_errors = serial.protocol_errors.saturating_add(1);
            return None;
        }
        Err(_) => {
            serial.protocol_errors = serial.protocol_errors.saturating_add(1);
            return None;
        }
    };

    let mut context = CommandContext::new(
        snapshot.current_page,
        snapshot.page_count,
        snapshot.last_error,
        snapshot.panel_mode as u8,
    );
    context.protocol_errors = serial.protocol_error_count();
    context.dropped_records = snapshot.dropped_log_count;
    let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
    let result = dispatch_command(
        header,
        &request_payload[..payload_len],
        &mut context,
        &mut response_payload,
    );
    match result {
        DispatchResult::RenderTurn { turn } => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::RenderTurn { turn },
        })),
        DispatchResult::RenderPage { target_page } => {
            Some(RuntimeCommand::Hardware(PendingCommand {
                header,
                action: PendingAction::RenderPage { target_page },
            }))
        }
        DispatchResult::DisplayProbe(probe) => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::DisplayProbe(probe),
        })),
        DispatchResult::CrashGet => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::CrashGet,
        })),
        DispatchResult::CrashClear => Some(RuntimeCommand::Hardware(PendingCommand {
            header,
            action: PendingAction::CrashClear,
        })),
        DispatchResult::LogGet { cursor, max_bytes } => Some(RuntimeCommand::LogGet {
            header,
            cursor,
            max_bytes,
        }),
        DispatchResult::LogClear => Some(RuntimeCommand::LogClear { header }),
        DispatchResult::NoAction => {
            let payload_len = if header.opcode == Opcode::Page {
                encode_page_response(snapshot.current_page, &mut response_payload).unwrap_or(0)
            } else {
                0
            };
            Some(RuntimeCommand::Immediate {
                header,
                status: Status::Ok,
                payload: response_payload,
                payload_len,
            })
        }
        DispatchResult::Response {
            status,
            payload_len,
        } => Some(RuntimeCommand::Immediate {
            header,
            status,
            payload: response_payload,
            payload_len,
        }),
    }
}

pub fn queue_runtime_response(
    serial: &mut SerialState,
    header: FrameHeader,
    status: Status,
    payload: &[u8],
) {
    build_response_frame(&header, status, payload, serial);
}

/// Parse one complete request. Immediate commands queue their response here;
/// hardware-backed commands return a token and intentionally queue nothing.
pub fn poll_pending_command<const N: usize>(
    serial: &mut SerialState,
    current_page: u32,
    page_count: u32,
    last_error: i32,
    panel_mode: u8,
    log: &mut DiagLog<N>,
    tick_ms: u32,
) -> Option<PendingCommand> {
    let mut frame_buf = [0u8; MAX_FRAME_BYTES];
    let frame_len = serial.next_frame(&mut frame_buf)?;
    let mut request_payload = [0u8; MAX_PAYLOAD_BYTES];
    let (header, payload_len) = match decode_frame(&frame_buf[..frame_len], &mut request_payload) {
        Ok(value) => value,
        Err(binbook_diagnostic_protocol::ProtocolError::UnknownOpcode) => {
            if let Ok((raw, _)) = decode_raw_frame(&frame_buf[..frame_len], &mut request_payload) {
                if raw.kind == FrameKind::Request as u8 {
                    let response = RawFrameHeader {
                        kind: FrameKind::Response as u8,
                        opcode: raw.opcode,
                        status: Status::BadRequest as u8,
                        sequence: raw.sequence,
                        payload_len: 0,
                    };
                    let mut encoded = [0u8; MAX_FRAME_BYTES];
                    if let Ok(len) = encode_raw_frame(&response, &[], &mut encoded) {
                        let _ = serial.queue_tx(&encoded[..len]);
                    }
                }
            }
            serial.protocol_errors = serial.protocol_errors.saturating_add(1);
            return None;
        }
        Err(_) => {
            serial.protocol_errors = serial.protocol_errors.saturating_add(1);
            return None;
        }
    };
    log.push(
        tick_ms,
        crate::diag_log::DiagEvent {
            level: crate::diag_log::LEVEL_INFO,
            subsystem: crate::diag_log::SUB_SERIAL,
            event: crate::diag_log::EVT_CMD_RECEIPT,
            arg0: header.opcode as i32,
            arg1: header.sequence as i32,
            arg2: 0,
        },
    );
    let mut context = CommandContext::new(current_page, page_count, last_error, panel_mode);
    context.protocol_errors = serial.protocol_error_count();
    context.dropped_records = log.dropped_records();
    context.tick_ms = tick_ms;
    let mut response_payload = [0u8; MAX_PAYLOAD_BYTES];
    let result = dispatch_command(
        header,
        &request_payload[..payload_len],
        &mut context,
        &mut response_payload,
    );
    if header.opcode == Opcode::Key {
        if let Ok(key) = decode_key_payload(&request_payload[..payload_len]) {
            log.push(
                tick_ms,
                crate::diag_log::DiagEvent {
                    level: crate::diag_log::LEVEL_INFO,
                    subsystem: crate::diag_log::SUB_INPUT,
                    event: crate::diag_log::EVT_KEY_PRESS,
                    arg0: key.key as i32,
                    arg1: key.action as i32,
                    arg2: 0,
                },
            );
        }
    }
    if header.opcode == Opcode::Page {
        let target = match result {
            DispatchResult::RenderPage { target_page } => target_page,
            DispatchResult::NoAction => current_page,
            _ => current_page,
        };
        log.push(
            tick_ms,
            crate::diag_log::DiagEvent {
                level: crate::diag_log::LEVEL_INFO,
                subsystem: crate::diag_log::SUB_NAV,
                event: crate::diag_log::EVT_PAGE_DECISION,
                arg0: current_page as i32,
                arg1: target as i32,
                arg2: 0,
            },
        );
    }
    let pending = match result {
        DispatchResult::RenderTurn { turn } => Some(PendingAction::RenderTurn { turn }),
        DispatchResult::RenderPage { target_page } => {
            Some(PendingAction::RenderPage { target_page })
        }
        DispatchResult::DisplayProbe(probe) => Some(PendingAction::DisplayProbe(probe)),
        DispatchResult::CrashGet => Some(PendingAction::CrashGet),
        DispatchResult::CrashClear => Some(PendingAction::CrashClear),
        DispatchResult::NoAction => {
            let payload_len = if header.opcode == Opcode::Page {
                encode_page_response(current_page, &mut response_payload).unwrap_or(0)
            } else {
                0
            };
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::LogGet { cursor, max_bytes } => {
            let payload_len = resolve_log_get(log, cursor, max_bytes, &mut response_payload);
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::LogClear => {
            let (next_cursor, dropped_log_count) = resolve_log_clear(log);
            let payload_len = encode_log_response_header(
                LogResponseHeader {
                    next_cursor,
                    dropped_log_count,
                    record_count: 0,
                },
                &mut response_payload,
            )
            .unwrap_or(0);
            build_response_frame(
                &header,
                Status::Ok,
                &response_payload[..payload_len],
                serial,
            );
            None
        }
        DispatchResult::Response {
            status,
            payload_len,
        } => {
            if status != Status::Ok {
                log.push(
                    tick_ms,
                    crate::diag_log::DiagEvent {
                        level: crate::diag_log::LEVEL_ERROR,
                        subsystem: crate::diag_log::SUB_SERIAL,
                        event: crate::diag_log::EVT_CMD_ERROR,
                        arg0: header.opcode as i32,
                        arg1: status as i32,
                        arg2: 0,
                    },
                );
            }
            build_response_frame(&header, status, &response_payload[..payload_len], serial);
            None
        }
    };
    pending.map(|action| PendingCommand { header, action })
}

pub fn complete_pending_command(
    serial: &mut SerialState,
    pending: PendingCommand,
    status: Status,
    resulting_page: u32,
    action_payload: &[u8],
) -> Result<(), TransportError> {
    let mut payload = [0u8; MAX_PAYLOAD_BYTES];
    let payload_len = if status != Status::Ok {
        0
    } else if pending.header.opcode == Opcode::Page {
        encode_page_response(resulting_page, &mut payload)
            .map_err(|_| TransportError::BufferFull)?
    } else {
        if action_payload.len() > payload.len() {
            return Err(TransportError::BufferFull);
        }
        payload[..action_payload.len()].copy_from_slice(action_payload);
        action_payload.len()
    };
    let response_header = FrameHeader {
        kind: FrameKind::Response,
        opcode: pending.header.opcode,
        status,
        sequence: pending.header.sequence,
        payload_len: payload_len as u16,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let encoded_len = encode_frame(&response_header, &payload[..payload_len], &mut encoded)
        .map_err(|_| TransportError::BufferFull)?;
    serial.queue_tx(&encoded[..encoded_len])
}

/// Returns `(next_cursor, dropped_log_count)` after clearing.
pub fn resolve_log_clear<const N: usize>(log: &mut DiagLog<N>) -> (u32, u32) {
    log.clear();
    (log.next_sequence(), log.dropped_records())
}

fn build_response_frame(
    header: &FrameHeader,
    status: Status,
    payload: &[u8],
    state: &mut SerialState,
) {
    let resp_header = FrameHeader {
        kind: FrameKind::Response,
        opcode: header.opcode,
        status,
        sequence: header.sequence,
        payload_len: payload.len() as u16,
    };
    let tx_len = encode_frame(&resp_header, payload, &mut state.tx_buf).unwrap_or(0);
    state.tx_pos = tx_len;
}
