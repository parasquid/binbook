#![cfg(feature = "diagnostic-console")]

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_summary_encode_writes_magic_version_and_crc() {
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary, CRASH_MAGIC};
    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 3,
        panel_mode: 1,
        boot_counter: 0,
        last_error: 4,
        last_page: 5,
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
            CrashLogSlot::default(),
        ],
    };
    let mut buf = [0xFFu8; 128];
    summary.encode(&mut buf);
    assert_eq!(&buf[..4], &CRASH_MAGIC);
    assert_eq!(buf[4], 1);
    assert_eq!(buf[6], 3);
    assert_eq!(buf[7], 1);
    assert_eq!(u32::from_le_bytes(buf[8..12].try_into().unwrap()), 0);
    assert_eq!(i32::from_le_bytes(buf[12..16].try_into().unwrap()), 4);
    assert_eq!(u32::from_le_bytes(buf[16..20].try_into().unwrap()), 5);
    assert_eq!(u32::from_le_bytes(buf[20..24].try_into().unwrap()), 100);
    let crc = u32::from_le_bytes(buf[124..128].try_into().unwrap());
    assert_ne!(crc, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_summary_decode_rejects_bad_magic() {
    use binbook_fw::diag_log::CrashSummary;
    let mut buf = [0xFFu8; 128];
    buf[0..4].copy_from_slice(b"BADD");
    let result = CrashSummary::decode(&buf);
    assert!(result.is_err());
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_summary_decode_rejects_bad_crc() {
    use binbook_fw::diag_log::{CrashLogSlot, CrashSummary};
    let summary = CrashSummary {
        flags: 0,
        copied_log_count: 1,
        panel_mode: 0,
        boot_counter: 0,
        last_error: 1,
        last_page: 2,
        last_log_sequence: 10,
        records: [CrashLogSlot::default(); 4],
    };
    let mut buf = [0u8; 128];
    summary.encode(&mut buf);
    buf[12] ^= 0xFF;
    let result = CrashSummary::decode(&buf);
    assert!(result.is_err());
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_crash_summary_decode_empty_sector_returns_none() {
    use binbook_fw::diag_log::CrashSummary;
    let buf = [0xFFu8; 128];
    let result = CrashSummary::decode(&buf);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}
