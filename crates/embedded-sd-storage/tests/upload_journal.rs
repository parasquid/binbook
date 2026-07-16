use embedded_sd_storage::{
    decode_upload_journal, encode_upload_journal, is_upload_journal_sector, UploadJournalPhase,
    UploadJournalRecord, UPLOAD_JOURNAL_BYTES,
};

#[test]
fn journal_layout_is_exact_crc_protected_and_roundtrips() {
    let record = UploadJournalRecord::new(
        UploadJournalPhase::OldMoved,
        0x12_345,
        1_022_165,
        0x0eea_9cb0,
        "book.binbook",
    )
    .unwrap();
    let mut encoded = [0xa5; UPLOAD_JOURNAL_BYTES];
    encode_upload_journal(&record, &mut encoded).unwrap();

    assert_eq!(&encoded[..4], b"BBXJ");
    assert_eq!(encoded[4], 1);
    assert_eq!(encoded[5], UploadJournalPhase::OldMoved as u8);
    assert_eq!(encoded[6], 12);
    assert_eq!(encoded[7], 0);
    assert_eq!(&encoded[8..12], &0x12_345u32.to_le_bytes());
    assert_eq!(&encoded[12..16], &1_022_165u32.to_le_bytes());
    assert_eq!(&encoded[16..20], &0x0eea_9cb0u32.to_le_bytes());
    assert_eq!(&encoded[20..32], b"book.binbook");
    assert!(encoded[32..144].iter().all(|byte| *byte == 0));
    assert_eq!(decode_upload_journal(&encoded).unwrap(), record);

    encoded[40] ^= 1;
    assert!(decode_upload_journal(&encoded).is_err());
}

#[test]
fn journal_sector_detection_requires_a_complete_crc_valid_record_at_sector_start() {
    let record = UploadJournalRecord::new(
        UploadJournalPhase::Verified,
        7,
        1024,
        0x1234_5678,
        "book.binbook",
    )
    .unwrap();
    let mut sector = [0xA5; 512];
    encode_upload_journal(&record, &mut sector[..UPLOAD_JOURNAL_BYTES]).unwrap();

    assert!(is_upload_journal_sector(&sector));
    sector[147] ^= 1;
    assert!(!is_upload_journal_sector(&sector));
    sector[147] ^= 1;
    sector.rotate_left(1);
    assert!(!is_upload_journal_sector(&sector));
}

#[test]
fn journal_rejects_invalid_phase_generation_size_name_reserved_and_length() {
    assert!(
        UploadJournalRecord::new(UploadJournalPhase::Receiving, 0, 1, 1, "book.binbook").is_err()
    );
    assert!(
        UploadJournalRecord::new(UploadJournalPhase::Receiving, 1, 0, 1, "book.binbook").is_err()
    );
    assert!(UploadJournalRecord::new(UploadJournalPhase::Receiving, 1, 1, 1, "bad/name").is_err());

    let record =
        UploadJournalRecord::new(UploadJournalPhase::Verified, 1, 1, 1, "book.binbook").unwrap();
    let mut encoded = [0; UPLOAD_JOURNAL_BYTES];
    encode_upload_journal(&record, &mut encoded).unwrap();
    encoded[141] = 1;
    assert!(decode_upload_journal(&encoded).is_err());
    assert!(decode_upload_journal(&encoded[..147]).is_err());
}
