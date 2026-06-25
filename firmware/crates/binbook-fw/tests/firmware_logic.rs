use binbook_fw::display::{
    build_display_smoke_row, decompress_row, logical_to_physical, stream_gray1_rows,
    DISPLAY_ROW_BYTES, GRAY1_ROW_BYTES,
};
use binbook_fw::flash::{FlashStorage, FILE_ENTRY_SIZE};
use binbook_fw::input::{decode_buttons, Button};
use binbook_fw::serial::{parse_command, Command};
use xteink_hal::{Flash, HalResult};

#[test]
fn decodes_adc_ladder_buttons() {
    assert_eq!(decode_buttons(500, 3000), Some(Button::Right));
    assert_eq!(decode_buttons(1000, 3000), Some(Button::Left));
    assert_eq!(decode_buttons(1800, 3000), Some(Button::Select));
    assert_eq!(decode_buttons(2300, 3000), Some(Button::Back));
    assert_eq!(decode_buttons(0, 1500), Some(Button::Up));
    assert_eq!(decode_buttons(0, 500), Some(Button::Down));
    assert_eq!(decode_buttons(0, 0), None);
}

#[test]
fn decompresses_binbook_packbits_variant_for_one_row() {
    let input = [
        0x01, 0xAA, 0xBB, // two literal bytes
        0x80, 0xCC, // repeat one byte; 0x80 is not a no-op
        0x82, 0xDD, // repeat three bytes
    ];
    let mut output = [0u8; 6];

    let consumed = decompress_row(&input, &mut output);

    assert_eq!(consumed, 7);
    assert_eq!(output, [0xAA, 0xBB, 0xCC, 0xDD, 0xDD, 0xDD]);
}

#[test]
fn maps_logical_portrait_coordinates_with_verified_x4_rotation() {
    assert_eq!(logical_to_physical(0, 0), (799, 0));
    assert_eq!(logical_to_physical(479, 0), (799, 479));
    assert_eq!(logical_to_physical(0, 799), (0, 0));
    assert_eq!(logical_to_physical(479, 799), (0, 479));
    assert_eq!(logical_to_physical(123, 456), (343, 123));
}

#[test]
fn streams_rows_from_packbits_runs_crossing_row_boundaries() {
    let input = [
        0xC1, 0xAA, // repeat 66 bytes: fills row 0 and starts row 1
        0xB5, 0x55, // repeat 54 bytes: finishes row 1
    ];

    let mut rows = Vec::new();
    stream_gray1_rows(&input, 2, |row_index, row| {
        rows.push((row_index, row.to_vec()));
        Ok::<(), ()>(())
    })
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], (0, vec![0xAA; GRAY1_ROW_BYTES]));

    let mut expected_second = vec![0xAA; 6];
    expected_second.extend_from_slice(&[0x55; 54]);
    assert_eq!(rows[1], (1, expected_second));
}

#[test]
fn builds_single_top_left_display_probe_rows() {
    let mut row = [0u8; DISPLAY_ROW_BYTES];

    build_display_smoke_row(0, &mut row);
    assert_eq!(&row[0..16], &[0x00; 16]);
    assert_eq!(&row[16..], &[0xFF; DISPLAY_ROW_BYTES - 16]);

    build_display_smoke_row(100, &mut row);
    assert!(row.iter().all(|byte| *byte == 0xFF));

    build_display_smoke_row(400, &mut row);
    assert!(row.iter().all(|byte| *byte == 0xFF));
}

#[test]
fn parses_serial_protocol_commands_without_allocation() {
    assert_eq!(parse_command("LIST"), Command::List);
    assert_eq!(parse_command("INFO"), Command::Info);
    assert_eq!(parse_command("PAGE"), Command::Page);
    assert_eq!(
        parse_command("UPLOAD sample.binbook 12345"),
        Command::Upload {
            name: "sample.binbook",
            size: 12345
        },
    );
    assert_eq!(
        parse_command("DELETE sample.binbook"),
        Command::Delete {
            name: "sample.binbook"
        },
    );
    assert_eq!(parse_command("UPLOAD bad-size nope"), Command::Unknown);
}

#[test]
fn serial_state_parses_command_split_across_chunks() {
    use binbook_fw::serial::SerialState;

    let mut state = SerialState::<64>::new();

    assert_eq!(state.feed(b"UPLO"), None);
    assert_eq!(
        state.feed(b"AD sample.binbook 4\n"),
        Some(Command::Upload {
            name: "sample.binbook",
            size: 4,
        })
    );
}

#[test]
fn finds_valid_flash_file_table_entry_by_name() {
    let mut flash = MockFlash::new();
    flash.write_entry(0, "sample.binbook", 512, 4096);

    let mut storage = FlashStorage::new(flash);
    let info = storage.find("sample.binbook").unwrap().unwrap();

    assert_eq!(info.offset, 512);
    assert_eq!(info.size, 4096);
}

#[test]
fn reads_file_bytes_relative_to_flash_file_offset() {
    let mut flash = MockFlash::new();
    flash.write_entry(0, "sample.binbook", 64, 4);
    flash.write(64, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

    let mut storage = FlashStorage::new(flash);
    let info = storage.find("sample.binbook").unwrap().unwrap();

    let mut out = [0u8; 2];
    storage.read_file(&info, 1, &mut out).unwrap();

    assert_eq!(out, [0xAD, 0xBE]);
}

#[test]
fn page_source_reads_exact_compressed_page_slice() {
    use binbook_fw::book::{PageExtent, PageSource};

    struct Bytes<'a>(&'a [u8]);

    impl PageSource for Bytes<'_> {
        type Error = ();

        fn read_at(&self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
            let offset = offset as usize;
            out.copy_from_slice(&self.0[offset..offset + out.len()]);
            Ok(())
        }
    }

    let source = Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let extent = PageExtent { offset: 2, size: 3 };
    let mut out = [0u8; 3];

    source.read_page(&extent, &mut out).unwrap();

    assert_eq!(out, [2, 3, 4]);
}

struct MockFlash {
    bytes: [u8; 512],
}

impl MockFlash {
    fn new() -> Self {
        Self { bytes: [0xFF; 512] }
    }

    fn write_entry(&mut self, index: usize, name: &str, offset: u32, size: u32) {
        let base = index * FILE_ENTRY_SIZE;
        let name_bytes = name.as_bytes();
        self.bytes[base..base + 32].fill(0);
        self.bytes[base..base + name_bytes.len()].copy_from_slice(name_bytes);
        self.bytes[base + 32..base + 36].copy_from_slice(&offset.to_le_bytes());
        self.bytes[base + 36..base + 40].copy_from_slice(&size.to_le_bytes());
        self.bytes[base + 40] = 0x00;
    }
}

impl Flash for MockFlash {
    fn read(&self, offset: u32, buf: &mut [u8]) -> HalResult<()> {
        let offset = offset as usize;
        buf.copy_from_slice(&self.bytes[offset..offset + buf.len()]);
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> HalResult<()> {
        let offset = offset as usize;
        self.bytes[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> HalResult<()> {
        let offset = offset as usize;
        self.bytes[offset..].fill(0xFF);
        Ok(())
    }

    fn size(&self) -> u32 {
        self.bytes.len() as u32
    }
}
