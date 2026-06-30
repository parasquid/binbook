#![cfg(feature = "diagnostic-console")]

struct CrashMockFlash {
    sector: [u8; 4096],
}

impl CrashMockFlash {
    fn new() -> Self {
        Self {
            sector: [0xFF; 4096],
        }
    }

    fn raw_bytes(&self) -> &[u8; 4096] {
        &self.sector
    }
}

impl embedded_storage::nor_flash::ErrorType for CrashMockFlash {
    type Error = core::convert::Infallible;
}

impl embedded_storage::nor_flash::ReadNorFlash for CrashMockFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        let base = (offset - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        buf.copy_from_slice(&self.sector[base..base + buf.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        binbook_fw::flash::CRASH_SECTOR_OFFSET as usize + self.sector.len()
    }
}

impl embedded_storage::nor_flash::NorFlash for CrashMockFlash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 4096;

    fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Self::Error> {
        let base = (offset - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        self.sector[base..base + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let base = (from - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        let end = (to - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        self.sector[base..end].fill(0xFF);
        Ok(())
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_summary_roundtrips_all_required_fields_and_four_records() {
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary, CRASH_RECORD_BYTES};

    let summary = CrashSummary {
        flags: 0x02,
        copied_log_count: 3,
        panel_mode: 1,
        boot_counter: 42,
        last_error: -12,
        last_page: 7,
        last_log_sequence: 100,
        records: [
            CrashLogSlot {
                sequence: 97,
                tick_ms: 1000,
                level: 2,
                subsystem: 3,
                event: 0x0010,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot {
                sequence: 98,
                tick_ms: 2000,
                level: 2,
                subsystem: 3,
                event: 0x0011,
                arg0: 1,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot {
                sequence: 99,
                tick_ms: 3000,
                level: 4,
                subsystem: 1,
                event: 0x0800,
                arg0: -1,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot {
                sequence: 100,
                tick_ms: 4000,
                level: 5,
                subsystem: 4,
                event: 0x0302,
                arg0: 99,
                arg1: 1,
                arg2: 2,
            },
        ],
    };

    let mut buf = [0u8; CRASH_RECORD_BYTES];
    summary.encode(&mut buf);
    let decoded = CrashSummary::decode(&buf).unwrap().unwrap();

    assert_eq!(decoded.flags, summary.flags);
    assert_eq!(decoded.copied_log_count, summary.copied_log_count);
    assert_eq!(decoded.panel_mode, summary.panel_mode);
    assert_eq!(decoded.boot_counter, summary.boot_counter);
    assert_eq!(decoded.last_error, summary.last_error);
    assert_eq!(decoded.last_page, summary.last_page);
    assert_eq!(decoded.last_log_sequence, summary.last_log_sequence);
    assert_eq!(decoded.records, summary.records);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_store_empty_flash_returns_none() {
    use binbook_fw::diag_flash::CrashStore;

    let flash = CrashMockFlash::new();
    let mut store = CrashStore::new(flash);
    let result = store.read().unwrap();
    assert!(result.is_none(), "fresh flash must return None");
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_store_survives_reopen() {
    use binbook_fw::diag_flash::CrashStore;
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};

    let summary = CrashSummary {
        flags: 0x01,
        copied_log_count: 2,
        panel_mode: 2,
        boot_counter: 99,
        last_error: -5,
        last_page: 3,
        last_log_sequence: 50,
        records: [
            CrashLogSlot {
                sequence: 48,
                tick_ms: 100,
                level: 1,
                subsystem: 0,
                event: 0x0001,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot {
                sequence: 49,
                tick_ms: 200,
                level: 3,
                subsystem: 1,
                event: 0x0302,
                arg0: 3,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot {
                sequence: 50,
                tick_ms: 300,
                level: 5,
                subsystem: 2,
                event: 0x0800,
                arg0: -5,
                arg1: 0,
                arg2: 0,
            },
            CrashLogSlot::default(),
        ],
    };

    let flash_bytes = {
        let flash = CrashMockFlash::new();
        let mut store = CrashStore::new(flash);
        store.write_fatal(&summary).unwrap();
        *store.flash().raw_bytes()
    };

    let mut flash2 = CrashMockFlash::new();
    flash2.sector = flash_bytes;
    let mut store2 = CrashStore::new(flash2);
    let recovered = store2.read().unwrap().unwrap();

    assert_eq!(recovered.flags, summary.flags);
    assert_eq!(recovered.copied_log_count, summary.copied_log_count);
    assert_eq!(recovered.panel_mode, summary.panel_mode);
    assert_eq!(recovered.boot_counter, summary.boot_counter);
    assert_eq!(recovered.last_error, summary.last_error);
    assert_eq!(recovered.last_page, summary.last_page);
    assert_eq!(recovered.last_log_sequence, summary.last_log_sequence);
    assert_eq!(recovered.records, summary.records);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_store_rejects_bad_crc() {
    use binbook_fw::diag_flash::CrashStore;
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};

    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 0,
        panel_mode: 0,
        boot_counter: 0,
        last_error: 1,
        last_page: 2,
        last_log_sequence: 10,
        records: [CrashLogSlot::default(); 4],
    };

    let flash = CrashMockFlash::new();
    let mut store = CrashStore::new(flash);
    store.write_fatal(&summary).unwrap();

    store.flash_mut().sector[12] ^= 0xFF;

    let result = store.read();
    assert!(result.is_err(), "corrupted CRC must return Err");
}
