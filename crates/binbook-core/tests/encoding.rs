use binbook_core::{
    ChapterIndexRecord, EncodeError, FileHeader, FontResourceIndexEntry, FontSourceKind, FontStyle,
    NavIndexRecord, PageChunkIndexRecord, PageIndexRecord, PageTransitionIndexRecord,
    PlaneDirectoryRecord, SectionTableEntry, StringRef, WireEncode, CHAPTER_INDEX_RECORD_SIZE,
    FONT_RESOURCE_RECORD_SIZE, HEADER_SIZE, NAV_INDEX_RECORD_SIZE, PAGE_CHUNK_INDEX_RECORD_SIZE,
    PAGE_INDEX_RECORD_SIZE, PAGE_TRANSITION_INDEX_RECORD_SIZE, SECTION_RECORD_SIZE,
};

#[test]
fn header_encoding_is_exact_little_endian_and_zero_filled() {
    let header = FileHeader {
        file_size: 0x0102_0304_0506_0708,
        section_table_offset: 256,
        section_table_length: 760,
        section_count: 19,
        page_data_offset: 65_536,
        page_data_length: 123_456,
        file_crc32: 0x1122_3344,
        header_crc32: 0x5566_7788,
        header_flags: 3,
    };
    let mut encoded = [0xaa_u8; HEADER_SIZE];

    header
        .encode_into(&mut encoded)
        .expect("header buffer has exact size");

    assert_eq!(&encoded[..8], b"BINBOOK\0");
    assert_eq!(&encoded[8..12], &[0, 0, 0, 0]);
    assert_eq!(&encoded[12..16], &[0, 1, 3, 0]);
    assert_eq!(&encoded[16..24], &header.file_size.to_le_bytes());
    assert_eq!(&encoded[24..32], &256_u64.to_le_bytes());
    assert_eq!(&encoded[32..36], &760_u32.to_le_bytes());
    assert_eq!(&encoded[36..44], &[40, 0, 19, 0, 128, 0, 48, 0]);
    assert_eq!(&encoded[44..52], &65_536_u64.to_le_bytes());
    assert_eq!(&encoded[52..60], &123_456_u64.to_le_bytes());
    assert_eq!(&encoded[60..64], &0x1122_3344_u32.to_le_bytes());
    assert_eq!(&encoded[64..68], &0x5566_7788_u32.to_le_bytes());
    assert!(encoded[68..].iter().all(|byte| *byte == 0));
}

#[test]
fn section_and_font_encoding_match_the_wire_layout() {
    let section = SectionTableEntry {
        section_id: 35,
        section_flags: 0,
        offset: 1024,
        length: 80,
        entry_size: 80,
        record_count: 1,
        crc32: 0xaabb_ccdd,
    };
    let mut section_bytes = [0xaa_u8; SECTION_RECORD_SIZE];
    section
        .encode_into(&mut section_bytes)
        .expect("section buffer has exact size");
    assert_eq!(&section_bytes[0..4], &[35, 0, 0, 0]);
    assert_eq!(&section_bytes[4..12], &1024_u64.to_le_bytes());
    assert_eq!(&section_bytes[12..20], &80_u64.to_le_bytes());
    assert_eq!(&section_bytes[20..24], &80_u32.to_le_bytes());
    assert_eq!(&section_bytes[24..28], &1_u32.to_le_bytes());
    assert_eq!(&section_bytes[28..32], &0xaabb_ccdd_u32.to_le_bytes());
    assert_eq!(&section_bytes[32..40], &[0; 8]);

    let font = FontResourceIndexEntry {
        font_index: 2,
        source_kind: FontSourceKind::Epub,
        flags: 0b1011,
        weight: 700,
        stretch_milli: 1_000,
        style: FontStyle::Italic,
        family: StringRef {
            offset: 4,
            length: 8,
        },
        source_path: StringRef {
            offset: 12,
            length: 16,
        },
        sha256: [0x5a; 32],
        face_index: 3,
    };
    let mut font_bytes = [0xaa_u8; FONT_RESOURCE_RECORD_SIZE];
    font.encode_into(&mut font_bytes)
        .expect("font buffer has exact size");
    assert_eq!(&font_bytes[0..4], &2_u32.to_le_bytes());
    assert_eq!(
        &font_bytes[4..16],
        &[2, 0, 11, 0, 188, 2, 232, 3, 1, 0, 0, 0]
    );
    assert_eq!(&font_bytes[16..24], &[4, 0, 0, 0, 8, 0, 0, 0]);
    assert_eq!(&font_bytes[24..32], &[12, 0, 0, 0, 16, 0, 0, 0]);
    assert_eq!(&font_bytes[32..64], &[0x5a; 32]);
    assert_eq!(&font_bytes[64..68], &3_u32.to_le_bytes());
    assert_eq!(&font_bytes[68..80], &[0; 12]);
}

#[test]
fn every_wire_encoder_reports_exact_required_and_provided_sizes() {
    let header = FileHeader::default();
    assert_eq!(
        header.encode_into(&mut [0_u8; HEADER_SIZE - 1]),
        Err(EncodeError::BufferTooSmall {
            required: HEADER_SIZE,
            provided: HEADER_SIZE - 1,
        })
    );

    let section = SectionTableEntry::default();
    assert_eq!(
        section.encode_into(&mut [0_u8; SECTION_RECORD_SIZE - 1]),
        Err(EncodeError::BufferTooSmall {
            required: SECTION_RECORD_SIZE,
            provided: SECTION_RECORD_SIZE - 1,
        })
    );

    for (actual, required) in [
        (
            PageIndexRecord::default().encode_into(&mut [0_u8; PAGE_INDEX_RECORD_SIZE - 1]),
            PAGE_INDEX_RECORD_SIZE,
        ),
        (
            NavIndexRecord::default().encode_into(&mut [0_u8; NAV_INDEX_RECORD_SIZE - 1]),
            NAV_INDEX_RECORD_SIZE,
        ),
        (
            ChapterIndexRecord::default().encode_into(&mut [0_u8; CHAPTER_INDEX_RECORD_SIZE - 1]),
            CHAPTER_INDEX_RECORD_SIZE,
        ),
        (
            PageChunkIndexRecord::default()
                .encode_into(&mut [0_u8; PAGE_CHUNK_INDEX_RECORD_SIZE - 1]),
            PAGE_CHUNK_INDEX_RECORD_SIZE,
        ),
        (
            PageTransitionIndexRecord::default()
                .encode_into(&mut [0_u8; PAGE_TRANSITION_INDEX_RECORD_SIZE - 1]),
            PAGE_TRANSITION_INDEX_RECORD_SIZE,
        ),
    ] {
        assert_eq!(
            actual,
            Err(EncodeError::BufferTooSmall {
                required,
                provided: required - 1,
            })
        );
    }

    let font = FontResourceIndexEntry {
        font_index: 0,
        source_kind: FontSourceKind::Bundled,
        flags: 0,
        weight: 400,
        stretch_milli: 1_000,
        style: FontStyle::Normal,
        family: StringRef::default(),
        source_path: StringRef::default(),
        sha256: [0; 32],
        face_index: 0,
    };
    assert_eq!(
        font.encode_into(&mut [0_u8; FONT_RESOURCE_RECORD_SIZE - 1]),
        Err(EncodeError::BufferTooSmall {
            required: FONT_RESOURCE_RECORD_SIZE,
            provided: FONT_RESOURCE_RECORD_SIZE - 1,
        })
    );
}

#[test]
fn page_and_auxiliary_record_encoders_match_wire_layouts() {
    let page = PageIndexRecord {
        page_number: 7,
        page_kind: 2,
        pixel_format: 1,
        compression_method: 3,
        update_hint: 4,
        page_flags: 0x1122_3344,
        page_crc32: 0x5566_7788,
        stored_width: 480,
        stored_height: 800,
        placement_x: 5,
        placement_y: 6,
        source_spine_index: 9,
        chapter_nav_index: 10,
        progress_start_ppm: 11,
        progress_end_ppm: 12,
        plane_directory: PlaneDirectoryRecord {
            bitmap: 0b1011,
            compression: [1, 2, 3, 4],
            offsets: [13, 14, 15, 16],
            sizes: [17, 18, 19, 20],
        },
    };
    let mut bytes = [0xaa; PAGE_INDEX_RECORD_SIZE];
    page.encode_into(&mut bytes).unwrap();
    assert_eq!(&bytes[0..4], &7_u32.to_le_bytes());
    assert_eq!(&bytes[4..12], &[2, 0, 1, 0, 3, 0, 4, 0]);
    assert_eq!(
        &bytes[12..20],
        &[0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55]
    );
    assert_eq!(&bytes[20..28], &[0xe0, 1, 0x20, 3, 5, 0, 6, 0]);
    assert_eq!(
        &bytes[28..44],
        &[9, 0, 0, 0, 10, 0, 0, 0, 11, 0, 0, 0, 12, 0, 0, 0]
    );
    assert_eq!(&bytes[44..52], &[0b1011, 1, 2, 3, 4, 0, 0, 0]);
    assert_eq!(
        &bytes[52..68],
        &[13, 0, 0, 0, 14, 0, 0, 0, 15, 0, 0, 0, 16, 0, 0, 0]
    );
    assert_eq!(
        &bytes[68..84],
        &[17, 0, 0, 0, 18, 0, 0, 0, 19, 0, 0, 0, 20, 0, 0, 0]
    );
    assert_eq!(&bytes[84..], &[0; 44]);

    let nav = NavIndexRecord {
        nav_index: 1,
        nav_type: 2,
        level: 3,
        title: StringRef {
            offset: 4,
            length: 5,
        },
        source_href: StringRef {
            offset: 6,
            length: 7,
        },
        source_spine_index: 8,
        target_page_number: 9,
        parent_nav_index: 10,
        first_child_nav_index: 11,
        next_sibling_nav_index: 12,
        nav_flags: 13,
    };
    let mut nav_bytes = [0; NAV_INDEX_RECORD_SIZE];
    nav.encode_into(&mut nav_bytes).unwrap();
    assert_eq!(&nav_bytes[0..8], &[1, 0, 0, 0, 2, 0, 3, 0]);
    assert_eq!(
        &nav_bytes[8..],
        &[
            4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 7, 0, 0, 0, 8, 0, 0, 0, 9, 0, 0, 0, 10, 0, 0, 0,
            11, 0, 0, 0, 12, 0, 0, 0, 13, 0, 0, 0
        ]
    );

    let chapter = ChapterIndexRecord {
        chapter_index: 1,
        nav_index: 2,
        title: StringRef {
            offset: 3,
            length: 4,
        },
        target_page_number: 5,
        level: 6,
        nav_type: 7,
        source_spine_index: 8,
        chapter_flags: 9,
    };
    let mut chapter_bytes = [0; CHAPTER_INDEX_RECORD_SIZE];
    chapter.encode_into(&mut chapter_bytes).unwrap();
    assert_eq!(
        &chapter_bytes,
        &[
            1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 7, 0, 8, 0, 0, 0, 9,
            0, 0, 0
        ]
    );

    let chunk = PageChunkIndexRecord {
        page_number: 1,
        plane_slot: 2,
        chunk_index: 3,
        row_start: 4,
        row_count: 5,
        page_data_offset: 6,
        compressed_size: 7,
        uncompressed_size: 8,
    };
    let mut chunk_bytes = [0xaa; PAGE_CHUNK_INDEX_RECORD_SIZE];
    chunk.encode_into(&mut chunk_bytes).unwrap();
    assert_eq!(
        &chunk_bytes,
        &[1, 0, 0, 0, 2, 3, 4, 0, 5, 0, 0, 0, 6, 0, 0, 0, 7, 0, 0, 0, 8, 0, 0, 0]
    );

    let transition = PageTransitionIndexRecord {
        from_page_number: 1,
        to_page_number: 2,
        changed_chunk_mask: 3,
        first_changed_chunk: 4,
        changed_chunk_count: 5,
        flags: 6,
    };
    let mut transition_bytes = [0xaa; PAGE_TRANSITION_INDEX_RECORD_SIZE];
    transition.encode_into(&mut transition_bytes).unwrap();
    assert_eq!(
        &transition_bytes,
        &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 5, 0, 6, 0, 0, 0, 0, 0, 0, 0]
    );
}
