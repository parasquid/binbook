mod common;

use std::io::Cursor;

use binbook_encode::PAGE_DATA_ALIGNMENT;
use sha2::{Digest, Sha256};

use common::builder;

const REQUIRED_SECTIONS: [u16; 19] = [
    1, 10, 11, 12, 20, 21, 22, 30, 31, 32, 33, 34, 35, 40, 41, 43, 44, 45, 50,
];

#[test]
fn writes_exact_order_alignment_indices_hashes_and_deterministic_bytes() {
    let builder = builder();
    let mut first = Cursor::new(Vec::new());
    let summary = builder.write_to(&mut first).unwrap();
    let bytes = first.into_inner();
    let mut second = Cursor::new(Vec::new());
    builder.write_to(&mut second).unwrap();
    assert_eq!(bytes, second.into_inner());
    assert_eq!(summary.page_count, 2);

    let table_offset =
        usize::try_from(u64::from_le_bytes(bytes[24..32].try_into().unwrap())).unwrap();
    let ids: Vec<u16> = (0..REQUIRED_SECTIONS.len())
        .map(|index| {
            u16::from_le_bytes(
                bytes[table_offset + index * 40..table_offset + index * 40 + 2]
                    .try_into()
                    .unwrap(),
            )
        })
        .collect();
    assert_eq!(ids, REQUIRED_SECTIONS);
    let page_data_offset =
        usize::try_from(u64::from_le_bytes(bytes[44..52].try_into().unwrap())).unwrap();
    assert_eq!(page_data_offset % PAGE_DATA_ALIGNMENT, 0);

    let (strings_offset, strings_len, _, _, _) = section(&bytes, 1);
    let strings = &bytes[strings_offset..strings_offset + strings_len];
    assert_eq!(
        strings
            .windows("Repeated".len())
            .filter(|value| *value == b"Repeated")
            .count(),
        1
    );

    for id in REQUIRED_SECTIONS {
        let (offset, length, _, _, expected_crc) = section(&bytes, id);
        assert_eq!(crc32(&bytes[offset..offset + length]), expected_crc);
    }

    let (font_offset, font_len, entry_size, count, _) = section(&bytes, 35);
    assert_eq!((entry_size, count, font_len), (80, 1, 80));
    let (policy_offset, _, _, _, _) = section(&bytes, 30);
    let expected_font_digest: [u8; 32] =
        Sha256::digest(&bytes[font_offset..font_offset + font_len]).into();
    assert_eq!(
        &bytes[policy_offset + 4..policy_offset + 36],
        &expected_font_digest
    );
    for (id, hash_offset) in [
        (10, 56),
        (11, 36),
        (30, 60),
        (31, 44),
        (32, 28),
        (33, 14),
        (34, 12),
    ] {
        let (offset, length, _, _, _) = section(&bytes, id);
        let mut unhashed = bytes[offset..offset + length].to_vec();
        let stored = unhashed[hash_offset..hash_offset + 32].to_vec();
        unhashed[hash_offset..hash_offset + 32].fill(0);
        assert_eq!(stored, Sha256::digest(&unhashed).as_slice());
    }

    let (rendition_offset, _, _, _, _) = section(&bytes, 22);
    let (source_offset, _, _, _, _) = section(&bytes, 20);
    let mut canonical = Vec::with_capacity(256);
    canonical.extend_from_slice(&bytes[source_offset + 28..source_offset + 60]);
    for (id, hash_offset) in [
        (10, 56),
        (11, 36),
        (30, 60),
        (31, 44),
        (32, 28),
        (33, 14),
        (34, 12),
    ] {
        let (offset, _, _, _, _) = section(&bytes, id);
        canonical.extend_from_slice(&bytes[offset + hash_offset..offset + hash_offset + 32]);
    }
    assert_eq!(
        &bytes[rendition_offset..rendition_offset + 32],
        Sha256::digest(&canonical).as_slice()
    );
    assert_eq!(
        &bytes[rendition_offset + 272..rendition_offset + 280],
        &[0; 8]
    );

    let (pages_offset, _, _, pages, _) = section(&bytes, 40);
    assert_eq!(pages, 2);
    assert_eq!(
        u32::from_le_bytes(
            bytes[pages_offset + 36..pages_offset + 40]
                .try_into()
                .unwrap()
        ),
        0
    );
    assert_eq!(
        u32::from_le_bytes(
            bytes[pages_offset + 40..pages_offset + 44]
                .try_into()
                .unwrap()
        ),
        500_000
    );
    assert_eq!(
        u32::from_le_bytes(
            bytes[pages_offset + 128 + 36..pages_offset + 128 + 40]
                .try_into()
                .unwrap()
        ),
        500_000
    );
    assert_eq!(
        u32::from_le_bytes(
            bytes[pages_offset + 128 + 40..pages_offset + 128 + 44]
                .try_into()
                .unwrap()
        ),
        1_000_000
    );
    let page_data = section(&bytes, 50).0;
    let bitmap = bytes[pages_offset + 44];
    assert_eq!(bitmap, 0x07);
    let mut crc_input = Vec::new();
    for slot in 0..3 {
        let relative = u32::from_le_bytes(
            bytes[pages_offset + 52 + slot * 4..pages_offset + 56 + slot * 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let length = u32::from_le_bytes(
            bytes[pages_offset + 68 + slot * 4..pages_offset + 72 + slot * 4]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(relative % 4, 0);
        crc_input.extend_from_slice(&bytes[page_data + relative..page_data + relative + length]);
    }
    assert_eq!(
        u32::from_le_bytes(
            bytes[pages_offset + 16..pages_offset + 20]
                .try_into()
                .unwrap()
        ),
        crc32(&crc_input)
    );

    let (chunks_offset, _, _, chunks, _) = section(&bytes, 44);
    let (transitions_offset, _, _, transitions, _) = section(&bytes, 45);
    assert_eq!(chunks, 180);
    assert_eq!(transitions, 2);
    for index in 0..90_usize {
        let record = chunks_offset + index * 24;
        let slot = usize::from(bytes[record + 4]);
        let chunk_offset = u32::from_le_bytes(bytes[record + 12..record + 16].try_into().unwrap());
        let chunk_size = u32::from_le_bytes(bytes[record + 16..record + 20].try_into().unwrap());
        let plane_offset = u32::from_le_bytes(
            bytes[pages_offset + 52 + slot * 4..pages_offset + 56 + slot * 4]
                .try_into()
                .unwrap(),
        );
        let plane_size = u32::from_le_bytes(
            bytes[pages_offset + 68 + slot * 4..pages_offset + 72 + slot * 4]
                .try_into()
                .unwrap(),
        );
        assert!(chunk_offset >= plane_offset);
        assert!(chunk_offset + chunk_size <= plane_offset + plane_size);
    }
    assert_eq!(
        &bytes[transitions_offset..transitions_offset + 8],
        &[0, 0, 0, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        &bytes[transitions_offset + 24..transitions_offset + 32],
        &[1, 0, 0, 0, 0, 0, 0, 0]
    );
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    crc ^ 0xffff_ffff
}

fn section(data: &[u8], id: u16) -> (usize, usize, u32, u32, u32) {
    let table = usize::try_from(u64::from_le_bytes(data[24..32].try_into().unwrap())).unwrap();
    let count = usize::from(u16::from_le_bytes(data[38..40].try_into().unwrap()));
    for index in 0..count {
        let record = table + index * 40;
        if u16::from_le_bytes(data[record..record + 2].try_into().unwrap()) == id {
            return (
                usize::try_from(u64::from_le_bytes(
                    data[record + 4..record + 12].try_into().unwrap(),
                ))
                .unwrap(),
                usize::try_from(u64::from_le_bytes(
                    data[record + 12..record + 20].try_into().unwrap(),
                ))
                .unwrap(),
                u32::from_le_bytes(data[record + 20..record + 24].try_into().unwrap()),
                u32::from_le_bytes(data[record + 24..record + 28].try_into().unwrap()),
                u32::from_le_bytes(data[record + 28..record + 32].try_into().unwrap()),
            );
        }
    }
    panic!("missing section {id}");
}
