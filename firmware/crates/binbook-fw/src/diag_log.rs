pub const DIAG_LOG_RECORDS: usize = 256;
pub const DEFAULT_LOG_CAPACITY: usize = DIAG_LOG_RECORDS;
pub const IDLE_SUMMARY_MS: u32 = 5000;
pub const ADC_SAMPLE_INTERVAL_MS: u32 = 100;

pub use binbook_diagnostic_protocol::{
    EVT_ADC_SAMPLE, EVT_BUTTON_EVENT, EVT_BW_BASE_SYNC_CANCELLED, EVT_BW_BASE_SYNC_COMPLETE,
    EVT_BW_BASE_SYNC_START, EVT_CMD_ERROR, EVT_CMD_RECEIPT, EVT_CONTROLLER_RAM_STATE,
    EVT_DISPLAY_ERROR, EVT_DISPLAY_RECOVERY, EVT_FIRMWARE_STARTED, EVT_GRAY_DELAY_CANCELLED,
    EVT_GRAY_OVERLAY_ACTIVATE, EVT_GRAY_OVERLAY_CANCELLED, EVT_GRAY_OVERLAY_COMPLETE,
    EVT_GRAY_OVERLAY_START, EVT_IDLE_ENTERED, EVT_IDLE_LEFT, EVT_IDLE_SUMMARY, EVT_INPUT_DECISION,
    EVT_INPUT_TRANSITION, EVT_KEY_PRESS, EVT_PAGE_DECISION, EVT_PAGE_TURN, EVT_PANEL_MODE,
    EVT_REFRESH_DECISION, EVT_REFRESH_PHASE, EVT_RENDER_FAILURE, EVT_RENDER_START,
    EVT_RENDER_SUCCESS, EVT_RESEED_COMPLETE, EVT_RESEED_START, EVT_TURN_BOUNDARY_NOOP,
    EVT_TURN_DEQUEUED, EVT_TURN_DROPPED, EVT_TURN_QUEUED, EVT_TURN_STARTED, EVT_WAVEFORM_SELECTED,
};

pub const LEVEL_TRACE: u8 = binbook_diagnostic_protocol::DiagLevelCode::Trace as u8;
pub const LEVEL_DEBUG: u8 = binbook_diagnostic_protocol::DiagLevelCode::Debug as u8;
pub const LEVEL_INFO: u8 = binbook_diagnostic_protocol::DiagLevelCode::Info as u8;
pub const LEVEL_WARN: u8 = binbook_diagnostic_protocol::DiagLevelCode::Warn as u8;
pub const LEVEL_ERROR: u8 = binbook_diagnostic_protocol::DiagLevelCode::Error as u8;

pub const SUB_SYSTEM: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::System as u8;
pub const SUB_INPUT: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Input as u8;
pub const SUB_NAV: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Navigation as u8;
pub const SUB_DISPLAY: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Display as u8;
pub const SUB_STORAGE: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Storage as u8;
pub const SUB_SERIAL: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Protocol as u8;
pub const SUB_IDLE: u8 = binbook_diagnostic_protocol::DiagSubsystemCode::Idle as u8;

pub const CRASH_MAGIC: [u8; 4] = *b"BBCR";
pub const CRASH_VERSION: u8 = 1;
pub const CRASH_RECORD_BYTES: usize = 128;
pub const CRASH_LOG_RECORDS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashSummaryError {
    BadMagic,
    BadCrc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashSummary {
    pub flags: u8,
    pub copied_log_count: u8,
    pub panel_mode: u8,
    pub boot_counter: u32,
    pub last_error: i32,
    pub last_page: u32,
    pub last_log_sequence: u32,
    pub records: [CrashLogSlot; CRASH_LOG_RECORDS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashLogSlot {
    pub sequence: u32,
    pub tick_ms: u32,
    pub level: u8,
    pub subsystem: u8,
    pub event: u16,
    pub arg0: i32,
    pub arg1: i32,
    pub arg2: i32,
}

impl CrashLogSlot {
    pub const fn default() -> Self {
        Self {
            sequence: 0,
            tick_ms: 0,
            level: 0,
            subsystem: 0,
            event: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }

    fn encode(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&self.sequence.to_le_bytes());
        buf[4..8].copy_from_slice(&self.tick_ms.to_le_bytes());
        buf[8] = self.level;
        buf[9] = self.subsystem;
        buf[10..12].copy_from_slice(&self.event.to_le_bytes());
        buf[12..16].copy_from_slice(&self.arg0.to_le_bytes());
        buf[16..20].copy_from_slice(&self.arg1.to_le_bytes());
        buf[20..24].copy_from_slice(&self.arg2.to_le_bytes());
    }

    fn decode(buf: &[u8]) -> Self {
        Self {
            sequence: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            tick_ms: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            level: buf[8],
            subsystem: buf[9],
            event: u16::from_le_bytes([buf[10], buf[11]]),
            arg0: i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            arg1: i32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            arg2: i32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
        }
    }
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

impl CrashSummary {
    pub fn encode(&self, buf: &mut [u8; CRASH_RECORD_BYTES]) {
        buf[0..4].copy_from_slice(&CRASH_MAGIC);
        buf[4] = CRASH_VERSION;
        buf[5] = self.flags;
        buf[6] = self.copied_log_count;
        buf[7] = self.panel_mode;
        buf[8..12].copy_from_slice(&self.boot_counter.to_le_bytes());
        buf[12..16].copy_from_slice(&self.last_error.to_le_bytes());
        buf[16..20].copy_from_slice(&self.last_page.to_le_bytes());
        buf[20..24].copy_from_slice(&self.last_log_sequence.to_le_bytes());
        for (i, slot) in self.records.iter().enumerate() {
            let offset = 24 + i * 24;
            slot.encode(&mut buf[offset..offset + 24]);
        }
        buf[120..124].fill(0);
        let crc = crc32_ieee(&buf[..124]);
        buf[124..128].copy_from_slice(&crc.to_le_bytes());
    }

    pub fn decode(buf: &[u8]) -> Result<Option<Self>, CrashSummaryError> {
        if buf.len() < CRASH_RECORD_BYTES {
            if buf.iter().all(|&b| b == 0xFF) {
                return Ok(None);
            }
            return Err(CrashSummaryError::BadMagic);
        }
        if buf[0..4] != CRASH_MAGIC {
            if buf.iter().all(|&b| b == 0xFF) {
                return Ok(None);
            }
            return Err(CrashSummaryError::BadMagic);
        }
        let stored_crc = u32::from_le_bytes([buf[124], buf[125], buf[126], buf[127]]);
        let computed_crc = crc32_ieee(&buf[..124]);
        if stored_crc != computed_crc {
            return Err(CrashSummaryError::BadCrc);
        }
        let mut records = [CrashLogSlot::default(); CRASH_LOG_RECORDS];
        for i in 0..CRASH_LOG_RECORDS {
            let offset = 24 + i * 24;
            records[i] = CrashLogSlot::decode(&buf[offset..offset + 24]);
        }
        Ok(Some(CrashSummary {
            flags: buf[5],
            copied_log_count: buf[6],
            panel_mode: buf[7],
            boot_counter: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            last_error: i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            last_page: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            last_log_sequence: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            records,
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagEvent {
    pub level: u8,
    pub subsystem: u8,
    pub event: u16,
    pub arg0: i32,
    pub arg1: i32,
    pub arg2: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagLogRecord {
    pub sequence: u32,
    pub tick_ms: u32,
    pub level: u8,
    pub subsystem: u8,
    pub event: u16,
    pub arg0: i32,
    pub arg1: i32,
    pub arg2: i32,
}

impl DiagLogRecord {
    pub const fn default() -> Self {
        Self {
            sequence: 0,
            tick_ms: 0,
            level: 0,
            subsystem: 0,
            event: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogReadResult {
    pub next_cursor: u32,
    pub dropped_log_count: u32,
    pub record_count: u16,
}

pub struct DiagLog<const N: usize> {
    records: [DiagLogRecord; N],
    write_index: usize,
    count: usize,
    next_sequence: u32,
    dropped: u32,
}

impl<const N: usize> DiagLog<N> {
    pub const fn new() -> Self {
        Self {
            records: [DiagLogRecord::default(); N],
            write_index: 0,
            count: 0,
            next_sequence: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, tick_ms: u32, event: DiagEvent) -> u32 {
        let seq = self.next_sequence;
        let record = DiagLogRecord {
            sequence: seq,
            tick_ms,
            level: event.level,
            subsystem: event.subsystem,
            event: event.event,
            arg0: event.arg0,
            arg1: event.arg1,
            arg2: event.arg2,
        };
        self.records[self.write_index] = record;
        self.write_index = (self.write_index + 1) % N;
        if self.count < N {
            self.count += 1;
        } else {
            self.dropped += 1;
        }
        self.next_sequence += 1;
        seq
    }

    pub fn push_event(&mut self, event: DiagEvent, tick_ms: u32) {
        self.push(tick_ms, event);
    }

    pub fn read_from_sequence(&self, cursor: u32, out: &mut [DiagLogRecord]) -> LogReadResult {
        let oldest = self.oldest_sequence().unwrap_or(self.next_sequence);
        let effective_cursor = cursor.max(oldest);

        if effective_cursor >= self.next_sequence {
            return LogReadResult {
                next_cursor: self.next_sequence,
                dropped_log_count: self.dropped,
                record_count: 0,
            };
        }

        let available = (self.next_sequence - effective_cursor) as usize;
        let to_copy = available.min(out.len());
        let start_offset = (effective_cursor - oldest) as usize;
        let start_index = (self.write_index + N - self.count + start_offset) % N;

        for i in 0..to_copy {
            out[i] = self.records[(start_index + i) % N];
        }

        LogReadResult {
            next_cursor: effective_cursor + to_copy as u32,
            dropped_log_count: self.dropped,
            record_count: to_copy as u16,
        }
    }

    pub fn oldest_sequence(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.next_sequence - self.count as u32)
        }
    }

    pub fn newest_sequence(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.next_sequence - 1)
        }
    }

    pub fn clear(&mut self) {
        self.write_index = 0;
        self.count = 0;
        self.dropped = 0;
    }

    pub fn dropped_records(&self) -> u32 {
        self.dropped
    }

    pub fn next_sequence(&self) -> u32 {
        self.next_sequence
    }

    pub fn record_count(&self) -> usize {
        self.count
    }

    pub fn copy_recent_crash_slots(&self, out: &mut [CrashLogSlot]) -> usize {
        let to_copy = self.count.min(out.len());
        let first = (self.write_index + N - to_copy) % N;
        for (index, slot) in out.iter_mut().take(to_copy).enumerate() {
            let record = self.records[(first + index) % N];
            *slot = CrashLogSlot {
                sequence: record.sequence,
                tick_ms: record.tick_ms,
                level: record.level,
                subsystem: record.subsystem,
                event: record.event,
                arg0: record.arg0,
                arg1: record.arg1,
                arg2: record.arg2,
            };
        }
        to_copy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdlePhase {
    Active,
    Idle,
}

pub struct DiagDeduper {
    last_idle_tick: u32,
    idle_suppressed: u32,
    phase: IdlePhase,
    last_summary_tick: u32,
}

impl DiagDeduper {
    pub fn new() -> Self {
        Self {
            last_idle_tick: 0,
            idle_suppressed: 0,
            phase: IdlePhase::Active,
            last_summary_tick: 0,
        }
    }

    pub fn push_enter_idle<const LOG_N: usize>(&mut self, log: &mut DiagLog<LOG_N>, tick_ms: u32) {
        self.phase = IdlePhase::Idle;
        self.last_idle_tick = tick_ms;
        self.last_summary_tick = tick_ms;
        self.idle_suppressed = 0;
        log.push(
            tick_ms,
            DiagEvent {
                level: 2,
                subsystem: 6,
                event: EVT_IDLE_ENTERED,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }

    pub fn push_idle_tick<const LOG_N: usize>(&mut self, log: &mut DiagLog<LOG_N>, tick_ms: u32) {
        let elapsed = tick_ms.saturating_sub(self.last_summary_tick);
        if elapsed >= IDLE_SUMMARY_MS {
            log.push(
                tick_ms,
                DiagEvent {
                    level: 2,
                    subsystem: 6,
                    event: EVT_IDLE_SUMMARY,
                    arg0: self.idle_suppressed as i32,
                    arg1: 0,
                    arg2: 0,
                },
            );
            self.idle_suppressed = 0;
            self.last_summary_tick = tick_ms;
        } else {
            self.idle_suppressed += 1;
        }
    }

    pub fn push_leave_idle<const LOG_N: usize>(&mut self, log: &mut DiagLog<LOG_N>, tick_ms: u32) {
        log.push(
            tick_ms,
            DiagEvent {
                level: 2,
                subsystem: 6,
                event: EVT_IDLE_LEFT,
                arg0: self.idle_suppressed as i32,
                arg1: 0,
                arg2: 0,
            },
        );
        self.phase = IdlePhase::Active;
        self.idle_suppressed = 0;
    }

    pub fn push_idle_or_summary<const LOG_N: usize>(
        &mut self,
        log: &mut DiagLog<LOG_N>,
        tick_ms: u32,
    ) {
        let elapsed = tick_ms.saturating_sub(self.last_idle_tick);
        if elapsed >= IDLE_SUMMARY_MS {
            log.push(
                tick_ms,
                DiagEvent {
                    level: 2,
                    subsystem: 6,
                    event: EVT_IDLE_SUMMARY,
                    arg0: self.idle_suppressed as i32,
                    arg1: 0,
                    arg2: 0,
                },
            );
            self.idle_suppressed = 0;
            self.last_idle_tick = tick_ms;
        } else {
            self.idle_suppressed += 1;
        }
    }
}
