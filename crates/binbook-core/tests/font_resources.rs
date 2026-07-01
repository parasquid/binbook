use binbook_core::{
    FontResourceIndexEntry, FontSourceKind, FontStyle, FormatError, StringRef,
    FONT_RESOURCE_RECORD_SIZE,
};

fn record_bytes() -> [u8; FONT_RESOURCE_RECORD_SIZE] {
    let mut bytes = [0_u8; FONT_RESOURCE_RECORD_SIZE];
    bytes[0..4].copy_from_slice(&0_u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&0b1011_u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&700_u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&1_000_u16.to_le_bytes());
    bytes[12] = 1;
    bytes[16..20].copy_from_slice(&4_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&8_u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&12_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&16_u32.to_le_bytes());
    for (index, byte) in bytes[32..64].iter_mut().enumerate() {
        *byte = u8::try_from(index).expect("digest index fits in u8");
    }
    bytes[64..68].copy_from_slice(&3_u32.to_le_bytes());
    bytes
}

#[test]
fn parses_font_resource_record_with_typed_fields() {
    let entry = FontResourceIndexEntry::parse(&record_bytes(), 0, 64)
        .expect("valid font resource must parse");

    assert_eq!(entry.font_index, 0);
    assert_eq!(entry.source_kind, FontSourceKind::Epub);
    assert_eq!(entry.flags, 0b1011);
    assert_eq!(entry.weight, 700);
    assert_eq!(entry.stretch_milli, 1_000);
    assert_eq!(entry.style, FontStyle::Italic);
    assert_eq!(
        entry.family,
        StringRef {
            offset: 4,
            length: 8
        }
    );
    assert_eq!(
        entry.source_path,
        StringRef {
            offset: 12,
            length: 16
        }
    );
    assert_eq!(entry.face_index, 3);
    assert_eq!(entry.sha256[0], 0);
    assert_eq!(entry.sha256[31], 31);
}

#[test]
fn rejects_invalid_font_resource_fields_and_reserved_bytes() {
    let mut invalid_source = record_bytes();
    invalid_source[4..6].copy_from_slice(&9_u16.to_le_bytes());
    assert_eq!(
        FontResourceIndexEntry::parse(&invalid_source, 0, 64),
        Err(FormatError::InvalidFontResource)
    );

    let mut invalid_flags = record_bytes();
    invalid_flags[6..8].copy_from_slice(&0x10_u16.to_le_bytes());
    assert_eq!(
        FontResourceIndexEntry::parse(&invalid_flags, 0, 64),
        Err(FormatError::InvalidFontResource)
    );

    let mut invalid_reserved = record_bytes();
    invalid_reserved[72] = 1;
    assert_eq!(
        FontResourceIndexEntry::parse(&invalid_reserved, 0, 64),
        Err(FormatError::InvalidFontResource)
    );
}

#[test]
fn rejects_wrong_index_and_out_of_bounds_string_refs() {
    assert_eq!(
        FontResourceIndexEntry::parse(&record_bytes(), 1, 64),
        Err(FormatError::InvalidFontResource)
    );

    assert_eq!(
        FontResourceIndexEntry::parse(&record_bytes(), 0, 20),
        Err(FormatError::InvalidStringRef)
    );
}
