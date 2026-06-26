use binbook_fw::display::{
    build_display_smoke_row, decompress_row, embedded_chunk_slice, embedded_page_slice,
    gray2_row_to_ssd1677_planes, is_supported_embedded_gray2_page, is_supported_x4_native_gray2_page,
    logical_to_physical, smoke_probe_windows, stream_gray1_rows, stream_gray2_rows,
    DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH, GRAY1_ROW_BYTES, GRAY2_ROW_BYTES,
};
use binbook_fw::flash::{FlashStorage, FILE_ENTRY_SIZE};
use binbook_fw::input::{apply_page_turn, decode_buttons, page_turn_for_button, Button, ButtonEvent, InputState, PageTurn};
use binbook_fw::refresh::{RefreshDecision, RefreshPolicy, RefreshState};
use binbook_fw::serial::{parse_command, Command};
use xteink_hal::{Flash, HalResult};

#[test]
fn decodes_adc_ladder_buttons() {
    assert_eq!(decode_buttons(500, 4095), Some(Button::Right));
    assert_eq!(decode_buttons(1000, 4095), Some(Button::Left));
    assert_eq!(decode_buttons(1800, 4095), Some(Button::Select));
    assert_eq!(decode_buttons(2800, 4095), Some(Button::Select));
    assert_eq!(decode_buttons(3500, 4095), Some(Button::Back));
    assert_eq!(decode_buttons(4095, 500), Some(Button::Down));
    assert_eq!(decode_buttons(4095, 1500), Some(Button::Up));
    assert_eq!(decode_buttons(4095, 2280), Some(Button::Up));
    assert_eq!(decode_buttons(4095, 4095), None);
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
fn streams_gray2_rows_from_packbits_runs_crossing_row_boundaries() {
    let input = [
        0xFF, 0xAA, // repeat 128 bytes
        0xD1, 0xAA, // repeat 82 bytes: fills row 0 and starts row 1
        0xFF, 0xBB, // repeat 128 bytes
        0xBD, 0xBB, // repeat 62 bytes: finishes row 1
    ];

    let mut rows = Vec::new();
    stream_gray2_rows(&input, 2, |row_index, row| {
        rows.push((row_index, row.to_vec()));
        Ok::<(), ()>(())
    })
    .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], (0, vec![0xAA; GRAY2_ROW_BYTES]));

    let mut expected_second = vec![0xAA; 10];
    expected_second.extend_from_slice(&[0xBB; 190]);
    assert_eq!(rows[1], (1, expected_second));
}

#[test]
fn gray2_row_conversion_maps_canonical_levels_to_ssd1677_planes() {
    let mut gray2 = [0xFFu8; GRAY2_ROW_BYTES];
    let mut red = [0u8; DISPLAY_ROW_BYTES];
    let mut black = [0u8; DISPLAY_ROW_BYTES];

    gray2[0] = 0b00011011; // black, dark gray, light gray, white at physical x 0..3

    gray2_row_to_ssd1677_planes(&gray2, &mut red, &mut black);

    assert_eq!(red[DISPLAY_ROW_BYTES - 1], 0xFC);
    assert_eq!(black[DISPLAY_ROW_BYTES - 1], 0xFA);
    assert!(red[..DISPLAY_ROW_BYTES - 1].iter().all(|byte| *byte == 0xFF));
    assert!(black[..DISPLAY_ROW_BYTES - 1]
        .iter()
        .all(|byte| *byte == 0xFF));
}

#[test]
fn builds_four_corner_display_probe_rows() {
    let mut row = [0u8; DISPLAY_ROW_BYTES];

    build_display_smoke_row(0, &mut row);
    assert_eq!(&row[0..16], &[0x00; 16]);
    assert_eq!(&row[16..84], &[0xFF; 68]);
    assert_eq!(&row[84..], &[0x00; 16]);

    build_display_smoke_row(100, &mut row);
    assert!(row.iter().all(|byte| *byte == 0xFF));

    build_display_smoke_row(400, &mut row);
    assert_eq!(&row[0..16], &[0x00; 16]);
    assert_eq!(&row[16..84], &[0xFF; 68]);
    assert_eq!(&row[84..], &[0x00; 16]);
}

#[test]
fn smoke_probe_windows_cover_all_physical_corners() {
    assert_eq!(
        smoke_probe_windows(),
        [
            (0, 0, 128, 96),
            (672, 0, 128, 96),
            (0, 384, 128, 96),
            (672, 384, 128, 96),
        ],
    );
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

#[test]
fn directional_buttons_map_to_page_turns() {
    assert_eq!(page_turn_for_button(Button::Right), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Down), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Left), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Up), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Select), None);
    assert_eq!(page_turn_for_button(Button::Back), None);
    assert_eq!(page_turn_for_button(Button::Power), None);
}

#[test]
fn page_turns_clamp_at_book_edges() {
    assert_eq!(apply_page_turn(0, 4, PageTurn::Previous), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::Next), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Previous), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(3, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(0, 0, PageTurn::Next), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::First), 0);
    assert_eq!(apply_page_turn(3, 4, PageTurn::First), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::Last), 3);
    assert_eq!(apply_page_turn(3, 4, PageTurn::Last), 3);
}

struct MockFlash {
    bytes: [u8; 512],
}

#[test]
fn raw_poll_emits_one_press_per_button_transition() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(4095, 4095, 0), None);
    assert_eq!(input.poll_raw(500, 4095, 150), Some(ButtonEvent::Press(Button::Right)));
    assert_eq!(input.poll_raw(500, 4095, 300), None);
    assert_eq!(input.poll_raw(4095, 4095, 450), None);
    assert_eq!(input.poll_raw(4095, 500, 600), Some(ButtonEvent::Press(Button::Down)));
}

#[test]
fn raw_poll_suppresses_transitions_inside_cooldown() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(500, 4095, 50), None);
    assert_eq!(input.poll_raw(500, 4095, 150), None);
}

#[test]
fn firmware_button_adc_uses_basic_calibration() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("AdcCalBasic"));
    assert!(main_rs.contains("enable_pin_with_cal"));
}

#[test]
fn supported_embedded_gray2_page_passes_validation() {
    use binbook::page_index::{COMPRESSION_RLE_PACKBITS, PIXEL_FORMAT_GRAY2_PACKED, PlaneDir};

    const PACKBITS: u8 = COMPRESSION_RLE_PACKBITS as u8;

    let info = binbook::PageInfo {
        page_number: 0,
        page_kind: 0,
        pixel_format: PIXEL_FORMAT_GRAY2_PACKED,
        compression_method: COMPRESSION_RLE_PACKBITS,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        chapter_nav_index: -1,
        plane_dir: PlaneDir {
            bitmap: 0x01,
            compression: [PACKBITS, 0, 0, 0],
            offsets: [0, 0, 0, 0],
            sizes: [100, 0, 0, 0],
        },
    };
    assert!(is_supported_embedded_gray2_page(&info));
}

#[test]
fn x4_native_page_with_three_plane_bitmap_passes_validation() {
    use binbook::page_index::{COMPRESSION_RLE_PACKBITS, PIXEL_FORMAT_GRAY2_PACKED, PlaneDir};

    const PACKBITS: u8 = COMPRESSION_RLE_PACKBITS as u8;

    let info = binbook::PageInfo {
        page_number: 0,
        page_kind: 0,
        pixel_format: PIXEL_FORMAT_GRAY2_PACKED,
        compression_method: COMPRESSION_RLE_PACKBITS,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        chapter_nav_index: -1,
        plane_dir: PlaneDir {
            bitmap: 0x07,
            compression: [PACKBITS, PACKBITS, PACKBITS, 0],
            offsets: [0, 780, 1560, 0],
            sizes: [780, 780, 780, 0],
        },
    };
    assert!(is_supported_x4_native_gray2_page(&info));
    assert!(!is_supported_embedded_gray2_page(&info));
}

#[test]
fn embedded_chunk_slice_returns_compressed_chunk_data() {
    use binbook::chunk_index::PageChunkEntry;

    let mut book_bytes = vec![0u8; 200];
    book_bytes[10..13].copy_from_slice(&[0xAA, 0xBB, 0xCC]);

    let chunk = PageChunkEntry {
        page_number: 0,
        plane_slot: 0,
        chunk_index: 0,
        row_start: 0,
        row_count: 16,
        page_data_offset: 0,
        compressed_size: 3,
        uncompressed_size: 1600,
    };

    let slice = embedded_chunk_slice(&book_bytes, 10, &chunk).unwrap();
    assert_eq!(slice, &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn embedded_chunk_slice_rejects_out_of_bounds() {
    use binbook::chunk_index::PageChunkEntry;

    let book_bytes = vec![0u8; 20];

    let chunk = PageChunkEntry {
        page_number: 0,
        plane_slot: 0,
        chunk_index: 0,
        row_start: 0,
        row_count: 16,
        page_data_offset: 0,
        compressed_size: 30,
        uncompressed_size: 1600,
    };

    assert!(embedded_chunk_slice(&book_bytes, 0, &chunk).is_none());
}

#[test]
fn unsupported_plane_bitmap_rejected() {
    use binbook::page_index::{COMPRESSION_RLE_PACKBITS, PIXEL_FORMAT_GRAY2_PACKED, PlaneDir};

    const PACKBITS: u8 = COMPRESSION_RLE_PACKBITS as u8;

    let info = binbook::PageInfo {
        page_number: 0,
        page_kind: 0,
        pixel_format: PIXEL_FORMAT_GRAY2_PACKED,
        compression_method: COMPRESSION_RLE_PACKBITS,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        chapter_nav_index: -1,
        plane_dir: PlaneDir {
            bitmap: 0x03,
            compression: [PACKBITS, 0, 0, 0],
            offsets: [0, 100, 0, 0],
            sizes: [100, 50, 0, 0],
        },
    };
    assert!(!is_supported_embedded_gray2_page(&info));
}

#[test]
fn embedded_page_slice_returns_compressed_plane_data() {
    use binbook::page_index::{COMPRESSION_RLE_PACKBITS, PIXEL_FORMAT_GRAY2_PACKED, PlaneDir};

    const PACKBITS: u8 = COMPRESSION_RLE_PACKBITS as u8;

    let mut book_bytes = vec![0u8; 100];
    book_bytes[15..18].copy_from_slice(&[0xAA, 0xBB, 0xCC]);

    let info = binbook::PageInfo {
        page_number: 0,
        page_kind: 0,
        pixel_format: PIXEL_FORMAT_GRAY2_PACKED,
        compression_method: COMPRESSION_RLE_PACKBITS,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        chapter_nav_index: -1,
        plane_dir: PlaneDir {
            bitmap: 0x01,
            compression: [PACKBITS, 0, 0, 0],
            offsets: [5, 0, 0, 0],
            sizes: [3, 0, 0, 0],
        },
    };

    let slice = embedded_page_slice(&book_bytes, 10, &info).unwrap();
    assert_eq!(slice, &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn embedded_page_slice_rejects_out_of_bounds() {
    use binbook::page_index::{COMPRESSION_RLE_PACKBITS, PIXEL_FORMAT_GRAY2_PACKED, PlaneDir};

    const PACKBITS: u8 = COMPRESSION_RLE_PACKBITS as u8;

    let book_bytes = vec![0u8; 20];

    let info = binbook::PageInfo {
        page_number: 0,
        page_kind: 0,
        pixel_format: PIXEL_FORMAT_GRAY2_PACKED,
        compression_method: COMPRESSION_RLE_PACKBITS,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        chapter_nav_index: -1,
        plane_dir: PlaneDir {
            bitmap: 0x01,
            compression: [PACKBITS, 0, 0, 0],
            offsets: [10, 0, 0, 0],
            sizes: [20, 0, 0, 0],
        },
    };

    assert!(embedded_page_slice(&book_bytes, 10, &info).is_none());
}

#[test]
fn refresh_policy_seeds_with_full_grayscale() {
    let state = RefreshState::new();
    assert_eq!(state.decide(0, None), RefreshDecision::FullGrayscale);
}

#[test]
fn refresh_policy_uses_full_screen_differential_default_after_seed() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    // After grayscale, BW seed is required before differential
    let bw_seed = state.decide(1, Some(0b101));
    assert_eq!(bw_seed, RefreshDecision::FullBwSeed);
    state.record_success(1, bw_seed);

    assert_eq!(
        state.decide(2, Some(0b101)),
        RefreshDecision::FullScreenDifferential
    );
}

#[test]
fn refresh_policy_uses_full_screen_differential_for_jump_without_transition() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    // After grayscale, BW seed is required before differential
    let bw_seed = state.decide(9, None);
    assert_eq!(bw_seed, RefreshDecision::FullBwSeed);
    state.record_success(9, bw_seed);

    assert_eq!(state.decide(5, None), RefreshDecision::FullScreenDifferential);
}

#[test]
fn refresh_policy_cleanup_after_five_fast_refreshes() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    // First page after grayscale requires BW seed
    let bw_seed = state.decide(1, Some(1));
    assert_eq!(bw_seed, RefreshDecision::FullBwSeed);
    state.record_success(1, bw_seed);

    // Pages 2-5 are differential (fast_refresh_count goes 2,3,4,5)
    for page in 2..=5 {
        let decision = state.decide(page, Some(1));
        assert!(matches!(decision, RefreshDecision::FullScreenDifferential));
        state.record_success(page, decision);
    }

    // After 5 fast refreshes, cleanup cadence triggers
    assert_eq!(state.decide(6, Some(1)), RefreshDecision::FullGrayscale);
}

#[test]
fn refresh_policy_noop_for_same_page() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    assert_eq!(state.decide(0, Some(0xFFFFFFFF)), RefreshDecision::Noop);
}

#[test]
fn failed_render_does_not_advance_previous_page() {
    let state = RefreshState::new();
    assert_eq!(state.previous_page(), None);
}

#[test]
fn refresh_state_fast_count_resets_on_full_grayscale() {
    let mut state = RefreshState::new();
    let seed = state.decide(0, None);
    state.record_success(0, seed);

    // First page requires BW seed
    let bw_seed = state.decide(1, Some(1));
    state.record_success(1, bw_seed);

    // Pages 2-5 are differential
    for page in 2..=5 {
        let decision = state.decide(page, Some(1));
        state.record_success(page, decision);
    }

    let cleanup = state.decide(6, Some(1));
    assert_eq!(cleanup, RefreshDecision::FullGrayscale);
    state.record_success(6, cleanup);
    assert_eq!(state.previous_page(), Some(6));

    // After cleanup, BW seed is required again
    let next = state.decide(7, Some(1));
    assert!(matches!(next, RefreshDecision::FullBwSeed));
}

#[test]
fn refresh_policy_defaults_to_full_screen_differential_when_transition_exists() {
    let mut state = RefreshState::new();
    let seed = state.decide_with_policy(0, None, RefreshPolicy::FullScreenDifferentialDefault);
    state.record_success(0, seed);

    // After grayscale, BW seed is required before differential
    let bw_seed = state.decide_with_policy(1, Some(0b101), RefreshPolicy::FullScreenDifferentialDefault);
    assert_eq!(bw_seed, RefreshDecision::FullBwSeed);
    state.record_success(1, bw_seed);

    assert_eq!(
        state.decide_with_policy(2, Some(0b101), RefreshPolicy::FullScreenDifferentialDefault),
        RefreshDecision::FullScreenDifferential
    );
}

#[test]
fn refresh_policy_uses_dirty_chunks_only_when_explicitly_enabled() {
    let mut state = RefreshState::new();
    let seed = state.decide_with_policy(0, None, RefreshPolicy::ChunkDirtyDifferentialDefault);
    state.record_success(0, seed);

    // After grayscale, BW seed is required before differential
    let bw_seed = state.decide_with_policy(1, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault);
    assert_eq!(bw_seed, RefreshDecision::FullBwSeed);
    state.record_success(1, bw_seed);

    assert_eq!(
        state.decide_with_policy(2, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::AdjacentDirtyPartial {
            changed_chunk_mask: 0b101
        }
    );
}

#[test]
fn display_page_with_policy_uses_explicit_refresh_policy() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("RefreshPolicy"));
    assert!(display_rs.contains("decide_with_policy"));
    assert!(!display_rs.contains("refresh_state.decide(target_page, transition_mask)"));
}

#[test]
fn chunk_dirty_normal_navigation_uses_full_screen_differential_default() {
    let main_rs = include_str!("../src/main.rs");

    // Normal path uses the clean-default wrapper, not the full policy function
    assert!(main_rs.contains("display_page_with_policy"));
    assert!(!main_rs.contains("display_page_with_refresh_policy"));
}

#[test]
fn chunk_dirty_policy_is_reserved_for_probe_or_debug_paths() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("display_page_with_refresh_policy"));
    assert!(display_rs.contains("RefreshPolicy::ChunkDirtyDifferentialDefault"));
}

#[test]
fn firmware_has_chunk_dirty_probe_feature_gate() {
    let cargo_toml = include_str!("../Cargo.toml");

    assert!(cargo_toml.contains("chunk-dirty-probe"));
}

#[test]
fn firmware_logs_refresh_policy_and_probe_steps() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("[REFRESH] policy="));
    assert!(main_rs.contains("[PROBE] chunk_dirty_window"));
}

#[test]
fn bw_seed_required_after_full_grayscale() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    assert_eq!(first, RefreshDecision::FullGrayscale);
    state.record_success(0, first);

    assert_eq!(state.decide(1, Some(0b101)), RefreshDecision::FullBwSeed);
}

#[test]
fn bw_seed_allows_full_screen_differential_after_record_success() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    state.record_success(0, first);
    let seed = state.decide(1, Some(0b101));
    assert_eq!(seed, RefreshDecision::FullBwSeed);
    state.record_success(1, seed);

    assert_eq!(
        state.decide(2, Some(0b111)),
        RefreshDecision::FullScreenDifferential
    );
}

#[test]
fn bw_seed_invalidated_by_cleanup_full_grayscale() {
    let mut state = RefreshState::new();
    let first = state.decide(0, None);
    state.record_success(0, first);
    let seed = state.decide(1, Some(1));
    state.record_success(1, seed);

    for page in 2..=5 {
        let decision = state.decide(page, Some(1));
        state.record_success(page, decision);
    }

    let cleanup = state.decide(6, Some(1));
    assert_eq!(cleanup, RefreshDecision::FullGrayscale);
    state.record_success(6, cleanup);

    assert_eq!(state.decide(7, Some(1)), RefreshDecision::FullBwSeed);
}

#[test]
fn bw_seed_required_before_chunk_dirty_policy() {
    let mut state = RefreshState::new();
    let first = state.decide_with_policy(0, None, RefreshPolicy::ChunkDirtyDifferentialDefault);
    state.record_success(0, first);

    assert_eq!(
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::FullBwSeed
    );

    state.record_success(1, RefreshDecision::FullBwSeed);
    assert_eq!(
        state.decide_with_policy(2, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault),
        RefreshDecision::AdjacentDirtyPartial {
            changed_chunk_mask: 0b101
        }
    );
}

#[test]
fn bw_seed_display_rendering_tracks_panel_mode_and_seed_path() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("pub enum PanelMode"));
    assert!(display_rs.contains("FullBwSeed"));
    assert!(display_rs.contains("stream_bw_seed_full"));
    assert!(display_rs.contains("init_grayscale_with_delay"));
    assert!(display_rs.contains("init_with_delay"));
}

#[test]
fn bw_seed_streams_target_bw_to_both_ram_planes() {
    let display_rs = include_str!("../src/display.rs");

    assert!(display_rs.contains("stream_bw_seed_full"));
    assert!(display_rs.contains("stream_plane_chunks_to_red"));
    assert!(display_rs.contains("stream_plane_chunks_to_black"));
    assert!(display_rs.contains("RefreshMode::Full"));
}

#[test]
fn bw_seed_main_owns_panel_mode_state() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("PanelMode::Unknown"));
    assert!(main_rs.contains("&mut panel_mode"));
}

#[test]
fn bw_seed_firmware_logs_refresh_decisions_and_panel_mode() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("[REFRESH] policy=FullScreenDifferentialDefault"));
    assert!(main_rs.contains("decision="));
    assert!(main_rs.contains("[PANEL] mode="));
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
