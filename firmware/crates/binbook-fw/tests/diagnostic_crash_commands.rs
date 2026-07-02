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
fn diag_crash_clear_erases_known_present_summary() {
    use binbook_fw::diag_flash::CrashStore;
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};

    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 1,
        panel_mode: 0,
        boot_counter: 0,
        last_error: 0,
        last_page: 0,
        last_log_sequence: 5,
        records: [CrashLogSlot::default(); 4],
    };

    let flash = CrashMockFlash::new();
    let mut store = CrashStore::new(flash);
    store.write_fatal(&summary).unwrap();
    assert!(
        store.read().unwrap().is_some(),
        "must be present before clear"
    );

    store.clear().unwrap();
    let after = store.read().unwrap();
    assert!(after.is_none(), "must be None after clear");
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_get_distinguishes_empty_and_present() {
    use binbook_fw::diag_flash::CrashStore;
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};

    let flash = CrashMockFlash::new();
    let mut store = CrashStore::new(flash);
    assert!(store.read().unwrap().is_none(), "empty flash returns None");

    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 0,
        panel_mode: 0,
        boot_counter: 0,
        last_error: 0,
        last_page: 0,
        last_log_sequence: 1,
        records: [CrashLogSlot::default(); 4],
    };
    store.write_fatal(&summary).unwrap();
    assert!(
        store.read().unwrap().is_some(),
        "written flash returns Some"
    );

    store.clear().unwrap();
    assert!(
        store.read().unwrap().is_none(),
        "cleared flash returns None again"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_clear_uses_distinct_opcode() {
    use binbook_diagnostic_protocol::{FrameHeader, FrameKind, Opcode, Status};
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::CrashClear,
        status: Status::Ok,
        sequence: 1,
        payload_len: 0,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let mut storage = binbook_fw::diag_storage::UnavailableStorage;
    let result = dispatch_command(header, &[], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(result, DispatchResult::CrashClear);

    let header_get = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::CrashGet,
        status: Status::Ok,
        sequence: 2,
        payload_len: 0,
    };
    let result_get = dispatch_command(header_get, &[], &mut ctx, &mut resp_buf, &mut storage);
    assert_eq!(result_get, DispatchResult::CrashGet);
    assert_ne!(result, result_get);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_sector_does_not_overlap_file_payload_region() {
    use binbook_fw::flash::{
        CRASH_SECTOR_OFFSET, CRASH_SECTOR_SIZE, FILE_ENTRY_SIZE, MAX_FILES, STORAGE_OFFSET,
    };

    let file_table_end = STORAGE_OFFSET + (MAX_FILES as u32) * (FILE_ENTRY_SIZE as u32);
    assert!(
        CRASH_SECTOR_OFFSET >= file_table_end,
        "crash sector at {:#X} must not overlap file table ending at {:#X}",
        CRASH_SECTOR_OFFSET,
        file_table_end,
    );
    assert_eq!(
        CRASH_SECTOR_OFFSET + CRASH_SECTOR_SIZE,
        STORAGE_OFFSET + 192 * 1024,
        "crash sector must be the final sector of the storage region"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_store_writes_only_on_fatal_or_explicit_clear() {
    use binbook_fw::diag_flash::CrashStore;
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};

    let flash = CrashMockFlash::new();
    let initial_sector = flash.sector;
    let mut store = CrashStore::new(flash);

    let _ = store.read().unwrap();
    assert_eq!(
        store.flash().raw_bytes(),
        &initial_sector,
        "read must not modify flash"
    );

    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 0,
        panel_mode: 0,
        boot_counter: 0,
        last_error: 0,
        last_page: 0,
        last_log_sequence: 0,
        records: [CrashLogSlot::default(); 4],
    };
    store.write_fatal(&summary).unwrap();
    assert_ne!(
        store.flash().raw_bytes(),
        &initial_sector,
        "write_fatal must modify flash"
    );

    store.clear().unwrap();
    assert_eq!(
        store.flash().raw_bytes(),
        &initial_sector,
        "clear must restore flash to erased state"
    );
}

#[cfg(all(feature = "diagnostic-console", feature = "debug-log"))]
#[test]
fn diag_crash_summary_copies_four_most_recent_records() {
    use binbook_fw::diag_log::{CrashLogSlot, DiagEvent, DiagLog};
    let mut log = DiagLog::<8>::new();
    for event in 0..6u16 {
        log.push(
            100 + event as u32,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event,
                arg0: event as i32,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut slots = [CrashLogSlot::default(); 4];
    let copied = log.copy_recent_crash_slots(&mut slots);
    assert_eq!(copied, 4);
    assert_eq!(slots.map(|slot| slot.sequence), [2, 3, 4, 5]);
    assert_eq!(slots.map(|slot| slot.event), [2, 3, 4, 5]);
}
