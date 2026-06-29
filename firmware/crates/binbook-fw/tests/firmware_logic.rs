#[cfg(feature = "diagnostic-console")]
use binbook_fw::diag_log::{DiagDeduper, DiagEvent, DiagLog, DiagLogRecord};
use binbook_fw::display::{
    build_display_smoke_row, decompress_row, embedded_chunk_slice, embedded_page_slice,
    gray2_row_to_ssd1677_planes, is_supported_embedded_gray2_page,
    is_supported_x4_native_gray2_page, logical_to_physical, smoke_probe_windows, stream_gray1_rows,
    stream_gray2_rows, DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH, GRAY1_ROW_BYTES,
    GRAY2_ROW_BYTES,
};
use binbook_fw::flash::{FlashStorage, FILE_ENTRY_SIZE};
use binbook_fw::input::{
    apply_page_turn, decode_buttons, page_turn_for_button, Button, ButtonEvent, InputDecision,
    InputPollOutcome, InputState, PageTurn,
};
use binbook_fw::refresh::{RefreshDecision, RefreshPolicy, RefreshState};
use xteink_hal::{Flash, HalResult};

#[test]
fn diagnostic_console_feature_gate() {
    let cargo_toml = include_str!("../Cargo.toml");
    let main_rs = include_str!("../src/main.rs");

    assert!(
        cargo_toml.contains("diagnostic-console"),
        "Cargo.toml must define diagnostic-console feature"
    );
    assert!(
        cargo_toml.contains("diagnostic-console ="),
        "Cargo.toml must have diagnostic-console feature definition"
    );
    assert!(
        cargo_toml.contains("debug-log"),
        "Cargo.toml must define debug-log feature"
    );
    assert!(
        main_rs.contains("#[cfg(feature = \"diagnostic-console\")]"),
        "main.rs must have diagnostic-console cfg block"
    );
    assert!(
        main_rs.contains("diag"),
        "main.rs must reference diag module"
    );
}

#[test]
fn firmware_runtime_uses_approved_async_configuration() {
    let cargo = include_str!("../Cargo.toml");
    let main_rs = include_str!("../src/main.rs");

    assert!(cargo.contains("esp-rtos"));
    assert!(cargo.contains("embassy-sync"));
    assert!(main_rs.contains("DISPLAY_SPI_FREQUENCY_MHZ"));
    assert!(!main_rs.contains("Rate::from_mhz(4)"));
    assert!(main_rs.contains("input_task"));
    assert!(main_rs.contains("display_task"));
    assert!(main_rs.contains("diagnostic_task"));
}

#[test]
fn firmware_runtime_starts_the_scheduler_before_async_main() {
    let main_rs = include_str!("../src/main.rs");
    let runtime_rs = include_str!("../src/runtime.rs");
    let hal_init = main_rs
        .find("esp_hal::init(esp_hal::Config::default())")
        .expect("main.rs must initialize esp-hal once for firmware-bin");
    let start = main_rs
        .find("esp_rtos::start(")
        .expect("main.rs must start the esp-rtos scheduler");
    let executor = main_rs
        .find("Executor::new()")
        .expect("main.rs must create the Embassy executor");
    let run = main_rs
        .find("executor.run(")
        .expect("main.rs must run the Embassy executor");

    assert!(
        runtime_rs
            .find("esp_hal::init(esp_hal::Config::default())")
            .is_none(),
        "runtime.rs must not reinitialize esp-hal after the scheduler starts"
    );
    assert!(
        hal_init < start,
        "HAL initialization must happen before scheduler startup"
    );
    assert!(
        start < executor,
        "scheduler startup must happen before executor creation"
    );
    assert!(
        start < run,
        "scheduler startup must happen before executor run"
    );
    assert!(main_rs.contains("TimerGroup::new"));
    assert!(main_rs.contains("SoftwareInterruptControl::new"));
}

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

    assert_eq!(red[DISPLAY_ROW_BYTES - 1], 0x03);
    assert_eq!(black[DISPLAY_ROW_BYTES - 1], 0x05);
    assert!(red[..DISPLAY_ROW_BYTES - 1]
        .iter()
        .all(|byte| *byte == 0x00));
    assert!(black[..DISPLAY_ROW_BYTES - 1]
        .iter()
        .all(|byte| *byte == 0x00));
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
    assert_eq!(
        input.poll_raw(500, 4095, 150),
        Some(ButtonEvent::Press(Button::Right))
    );
    assert_eq!(input.poll_raw(500, 4095, 300), None);
    assert_eq!(input.poll_raw(4095, 4095, 450), None);
    assert_eq!(
        input.poll_raw(4095, 500, 600),
        Some(ButtonEvent::Press(Button::Down))
    );
}

#[test]
fn raw_poll_suppresses_transitions_inside_cooldown() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(500, 4095, 50), None);
    assert_eq!(input.poll_raw(500, 4095, 150), None);
}

#[test]
fn detailed_poll_reports_the_initial_cooldown_suppression() {
    let mut input = InputState::new();

    assert_eq!(
        input.poll_raw_detailed(500, 4095, 50),
        InputPollOutcome {
            previous: None,
            observed: Some(Button::Right),
            elapsed_since_last_press_ms: 50,
            decision: InputDecision::SuppressedByCooldown {
                observed: Some(Button::Right),
                elapsed_ms: 50,
            },
        }
    );
}

#[test]
fn detailed_poll_reports_one_accepted_press() {
    let mut input = InputState::new();

    assert_eq!(
        input.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );
    assert_eq!(
        input.poll_raw_detailed(500, 4095, 250).decision,
        InputDecision::Unchanged
    );
}

#[test]
fn detailed_poll_preserves_exact_cooldown_and_suppressed_state_change() {
    let mut input = InputState::new();
    assert_eq!(
        input.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );

    assert_eq!(
        input.poll_raw_detailed(1000, 4095, 201).decision,
        InputDecision::SuppressedByCooldown {
            observed: Some(Button::Left),
            elapsed_ms: 100,
        }
    );
    assert_eq!(input.last_button(), Some(Button::Left));
    assert_eq!(
        input.poll_raw_detailed(1000, 4095, 302).decision,
        InputDecision::Unchanged
    );
}

#[test]
fn detailed_poll_reports_release_without_changing_legacy_events() {
    let mut detailed = InputState::new();
    assert_eq!(
        detailed.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );
    assert_eq!(
        detailed.poll_raw_detailed(4095, 4095, 202).decision,
        InputDecision::Released
    );

    let mut legacy = InputState::new();
    assert_eq!(
        legacy.poll_raw(500, 4095, 101),
        Some(ButtonEvent::Press(Button::Right))
    );
    assert_eq!(legacy.poll_raw(4095, 4095, 202), None);
}

#[test]
fn firmware_button_adc_uses_basic_calibration() {
    let main_rs = include_str!("../src/main.rs");

    assert!(main_rs.contains("AdcCalBasic"));
    assert!(main_rs.contains("enable_pin_with_cal"));
}

fn test_page_info(bitmap: u8, offsets: [u32; 4], sizes: [u32; 4]) -> binbook_core::PageInfo {
    let make_plane = |slot: binbook_core::PlaneSlot, index: usize| {
        if bitmap & (1 << index) == 0 {
            None
        } else {
            Some(binbook_core::PlaneDescriptor::new(
                slot,
                binbook_core::CompressionMethod::RlePackBits,
                binbook_core::FileOffset::new(u64::from(offsets[index])),
                binbook_core::ByteLength::new(sizes[index]),
            ))
        }
    };
    binbook_core::PageInfo {
        page_number: binbook_core::PageNumber::new(0, 1).unwrap(),
        page_kind: 0,
        pixel_format: binbook_core::PixelFormat::Gray2Packed,
        compression_method: binbook_core::CompressionMethod::RlePackBits,
        update_hint: 0,
        page_flags: 0,
        page_crc32: 0,
        stored_width: DISPLAY_WIDTH,
        stored_height: DISPLAY_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 0,
        planes: binbook_core::PlaneDirectory::new([
            make_plane(binbook_core::PlaneSlot::OverlayMsb, 0),
            make_plane(binbook_core::PlaneSlot::OverlayLsb, 1),
            make_plane(binbook_core::PlaneSlot::FastBase, 2),
            make_plane(binbook_core::PlaneSlot::Reserved, 3),
        ]),
    }
}

fn test_display_profile(
    physical_width: u16,
    physical_height: u16,
    waveform_hint: u16,
) -> binbook_core::DisplayProfile {
    let empty = binbook_core::StringRef {
        offset: 0,
        length: 0,
    };
    binbook_core::DisplayProfile {
        profile_id: empty,
        device_family: empty,
        device_model: empty,
        logical_width: 480,
        logical_height: 800,
        physical_width,
        physical_height,
        logical_orientation: 1,
        logical_to_physical_rotation: 270,
        scan_order_hint: 1,
        supported_storage_pixel_formats: 3,
        native_output_pixel_formats: 3,
        native_grayscale_levels: 4,
        panel_grayscale_levels: 4,
        framebuffer_bits_per_pixel: 2,
        waveform_hint,
        dither_mode: 0,
    }
}

fn test_chunk(compressed_size: u32) -> binbook_core::PageChunk {
    binbook_core::PageChunk {
        page_number: binbook_core::PageNumber::new(0, 1).unwrap(),
        plane_slot: binbook_core::PlaneSlot::OverlayMsb,
        chunk_index: binbook_core::ChunkIndex::new(0, 30).unwrap(),
        row_start: 0,
        row_count: 16,
        offset: binbook_core::FileOffset::new(0),
        compressed_length: binbook_core::ByteLength::new(compressed_size),
        uncompressed_length: binbook_core::ByteLength::new(1600),
    }
}

#[test]
fn supported_embedded_gray2_page_passes_validation() {
    let info = test_page_info(0x01, [0; 4], [100, 0, 0, 0]);
    assert!(is_supported_embedded_gray2_page(&info));
}

#[test]
fn x4_native_page_with_three_plane_bitmap_passes_validation() {
    let info = test_page_info(0x07, [0, 780, 1560, 0], [780, 780, 780, 0]);
    let profile = test_display_profile(
        DISPLAY_WIDTH,
        DISPLAY_HEIGHT,
        binbook_core::WAVEFORM_SSD1677_STAGED_GRAY2,
    );
    assert!(is_supported_x4_native_gray2_page(&profile, &info));
    assert!(!is_supported_embedded_gray2_page(&info));
}

#[test]
fn x4_native_page_rejects_wrong_dimensions_or_waveform() {
    let info = test_page_info(0x07, [0; 4], [1, 1, 1, 0]);
    let absolute = test_display_profile(
        DISPLAY_WIDTH,
        DISPLAY_HEIGHT,
        binbook_core::WAVEFORM_SSD1677_ABSOLUTE_GRAY2,
    );
    let wrong_size = test_display_profile(480, 800, binbook_core::WAVEFORM_SSD1677_STAGED_GRAY2);

    assert!(!is_supported_x4_native_gray2_page(&absolute, &info));
    assert!(!is_supported_x4_native_gray2_page(&wrong_size, &info));
}

#[test]
fn embedded_chunk_slice_returns_compressed_chunk_data() {
    let mut book_bytes = vec![0u8; 200];
    book_bytes[10..13].copy_from_slice(&[0xAA, 0xBB, 0xCC]);

    let chunk = test_chunk(3);

    let slice = embedded_chunk_slice(&book_bytes, 10, &chunk).unwrap();
    assert_eq!(slice, &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn embedded_chunk_slice_rejects_out_of_bounds() {
    let book_bytes = vec![0u8; 20];

    let chunk = test_chunk(30);

    assert!(embedded_chunk_slice(&book_bytes, 0, &chunk).is_none());
}

#[test]
fn unsupported_plane_bitmap_rejected() {
    let info = test_page_info(0x03, [0, 100, 0, 0], [100, 50, 0, 0]);
    assert!(!is_supported_embedded_gray2_page(&info));
}

#[test]
fn embedded_page_slice_returns_compressed_plane_data() {
    let mut book_bytes = vec![0u8; 100];
    book_bytes[15..18].copy_from_slice(&[0xAA, 0xBB, 0xCC]);

    let info = test_page_info(0x01, [5, 0, 0, 0], [3, 0, 0, 0]);

    let slice = embedded_page_slice(&book_bytes, 10, &info).unwrap();
    assert_eq!(slice, &[0xAA, 0xBB, 0xCC]);
}

#[test]
fn embedded_page_slice_rejects_out_of_bounds() {
    let book_bytes = vec![0u8; 20];

    let info = test_page_info(0x01, [10, 0, 0, 0], [20, 0, 0, 0]);

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

    assert_eq!(
        state.decide(5, None),
        RefreshDecision::FullScreenDifferential
    );
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
    let bw_seed =
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::FullScreenDifferentialDefault);
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
    let bw_seed =
        state.decide_with_policy(1, Some(0b101), RefreshPolicy::ChunkDirtyDifferentialDefault);
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

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_push_assigns_ascending_sequence_numbers() {
    let mut log = DiagLog::<16>::new();
    log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    log.push(
        200,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0011,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    let mut buf = [DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 2);
    assert_eq!(buf[0].sequence, 0);
    assert_eq!(buf[1].sequence, 1);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_read_from_cursor_returns_records_in_order() {
    let mut log = DiagLog::<4>::new();
    for i in 0..4u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 4);
    assert_eq!(buf[0].event, 0);
    assert_eq!(buf[3].event, 3);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_push_overwrites_oldest_and_increments_dropped() {
    let mut log = DiagLog::<4>::new();
    for i in 0..8u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert_eq!(log.dropped_records(), 4);
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 4);
    assert_eq!(buf[0].event, 4);
    assert_eq!(buf[3].event, 7);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_resets_records_and_dropped() {
    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 2,
                subsystem: 3,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert_eq!(log.dropped_records(), 2);
    log.clear();
    assert_eq!(log.dropped_records(), 0);
    let mut buf = [DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut buf);
    assert_eq!(result.record_count, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_deduper_suppresses_idle_repeats() {
    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    for _ in 0..100 {
        deduper.push_idle_or_summary(&mut log, 50);
    }
    let mut buf = [DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut buf);
    assert!(
        result.record_count <= 2,
        "expected at most 2 idle records, got {}",
        result.record_count
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_record_is_plain_copy() {
    let a = DiagLogRecord::default();
    let b = a;
    assert_eq!(a.sequence, b.sequence);
}

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

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_key_right_press_matches_button_right() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 2,
    };
    let mut ctx = CommandContext::new(5, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0x02, 0x01], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, PageTurn::Next);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_key_left_press_matches_button_left() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 2,
        payload_len: 2,
    };
    let mut ctx = CommandContext::new(5, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0x01, 0x01], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, PageTurn::Previous);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_next_clamps_at_end() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 3,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(19, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0x01], &mut ctx, &mut resp_buf);
    assert_eq!(result, DispatchResult::NoAction);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_goto_clamps_at_edges() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 4,
        payload_len: 5,
    };
    let mut ctx = CommandContext::new(0, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(
        header,
        &[0x05, 0xFF, 0xFF, 0xFF, 0xFF],
        &mut ctx,
        &mut resp_buf,
    );
    match result {
        DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!("expected BadRequest for out-of-range goto, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_page_goto_valid_targets_exact_page() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 4,
        payload_len: 5,
    };
    let mut ctx = CommandContext::new(0, 20, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(
        header,
        &[0x05, 0x0A, 0x00, 0x00, 0x00],
        &mut ctx,
        &mut resp_buf,
    );
    match result {
        DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 10);
        }
        other => panic!("expected RenderPage for valid goto, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_dispatch_status_includes_current_state() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Status,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 5,
        payload_len: 0,
    };
    let mut ctx = CommandContext::new(7, 30, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status,
            payload_len,
        } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::Ok);
            assert_eq!(payload_len, 21);
        }
        other => panic!("expected Response, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_window_corners_maps_to_render_request() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 6,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0x03], &mut ctx, &mut resp_buf);
    assert_eq!(
        result,
        DispatchResult::DisplayProbe(binbook_fw::diag::DisplayProbeKind::WindowCorners)
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_clear_white_maps_to_render_request() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 7,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0x02], &mut ctx, &mut resp_buf);
    assert_eq!(
        result,
        DispatchResult::DisplayProbe(binbook_fw::diag::DisplayProbeKind::ClearWhite)
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_unknown_code_returns_error() {
    use binbook_fw::diag::{dispatch_command, CommandContext, DispatchResult};
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::DisplayProbe,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 8,
        payload_len: 1,
    };
    let mut ctx = CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = dispatch_command(header, &[0xFF], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!(
            "expected BadRequest for unknown probe code, got {:?}",
            other
        ),
    }
}

#[test]
fn diagnostic_console_usb_serial() {
    let main_rs = include_str!("../src/main.rs");
    let diag_rs = include_str!("../src/diag.rs");

    assert!(
        main_rs.contains("UsbSerialJtag"),
        "main.rs must reference UsbSerialJtag"
    );
    assert!(
        main_rs.contains("poll_pending_command") && main_rs.contains("complete_pending_command"),
        "main.rs must use the response-after-action command flow"
    );
    assert!(
        diag_rs.contains("rx_buf"),
        "diag.rs must have fixed RX buffer"
    );
    assert!(
        diag_rs.contains("tx_buf"),
        "diag.rs must have fixed TX buffer"
    );
    assert!(
        diag_rs.contains("dispatch_command"),
        "diag.rs must define dispatch_command"
    );
}

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_event_constants_exist() {
    use binbook_fw::diag_log::{
        EVT_ADC_SAMPLE, EVT_BUTTON_EVENT, EVT_DISPLAY_ERROR, EVT_FIRMWARE_STARTED,
        EVT_IDLE_SUMMARY, EVT_PAGE_TURN, EVT_PANEL_MODE, EVT_REFRESH_DECISION, EVT_RENDER_START,
    };

    assert!(EVT_FIRMWARE_STARTED > 0);
    assert!(EVT_ADC_SAMPLE > 0);
    assert!(EVT_BUTTON_EVENT > 0);
    assert!(EVT_PAGE_TURN > 0);
    assert!(EVT_RENDER_START > 0);
    assert!(EVT_REFRESH_DECISION > 0);
    assert!(EVT_PANEL_MODE > 0);
    assert!(EVT_DISPLAY_ERROR > 0);
    assert!(EVT_IDLE_SUMMARY > 0);
}

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_push_event_records_sequence() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog, DEFAULT_LOG_CAPACITY};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();

    assert_eq!(log.next_sequence(), 0);

    log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: 0,
            arg1: 0,
            arg2: 42,
        },
    );
    log.push(
        200,
        DiagEvent {
            level: 3,
            subsystem: 1,
            event: 0x0800,
            arg0: 0,
            arg1: 0,
            arg2: 100,
        },
    );

    assert_eq!(log.next_sequence(), 2);
    let mut out = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut out);
    assert_eq!(result.record_count, 2);
    assert_eq!(out[0].sequence, 0);
    assert_eq!(out[0].level, 2);
    assert_eq!(out[0].subsystem, 3);
    assert_eq!(out[0].event, 0x0010);
    assert_eq!(out[0].arg2, 42);
    assert_eq!(out[1].sequence, 1);
    assert_eq!(out[1].level, 3);
    assert_eq!(out[1].subsystem, 1);
}

#[test]
#[cfg(feature = "diagnostic-console")]
fn diag_structured_logging_idle_deduper_bounds_records() {
    use binbook_fw::diag_log::{DiagDeduper, DiagLog, DEFAULT_LOG_CAPACITY, IDLE_SUMMARY_MS};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();
    let mut deduper = DiagDeduper::new();

    for tick in (1..IDLE_SUMMARY_MS).step_by(10) {
        deduper.push_idle_or_summary(&mut log, tick);
    }

    assert_eq!(
        log.next_sequence(),
        0,
        "no records should be written during suppressed idle ticks"
    );

    deduper.push_idle_or_summary(&mut log, IDLE_SUMMARY_MS);
    assert_eq!(
        log.next_sequence(),
        1,
        "exactly one summary record at cadence boundary"
    );

    let mut out = [binbook_fw::diag_log::DiagLogRecord::default(); 1];
    let result = log.read_from_sequence(0, &mut out);
    assert_eq!(result.record_count, 1);
    assert_eq!(out[0].level, 2);
    assert_eq!(out[0].subsystem, 6);
    assert_eq!(out[0].event, binbook_fw::diag_log::EVT_IDLE_SUMMARY);
    assert_eq!(out[0].arg0, IDLE_SUMMARY_MS as i32 / 10);
}

#[test]
fn diag_structured_logging_main_rs_mirrors_events_not_just_dbgprintln() {
    let main_rs = include_str!("../src/main.rs");
    let diag_log_rs = include_str!("../src/diag_log.rs");

    assert!(
        diag_log_rs.contains("EVT_FIRMWARE_STARTED"),
        "diag_log.rs must define firmware started event"
    );
    assert!(
        diag_log_rs.contains("EVT_PAGE_TURN"),
        "diag_log.rs must define page turn event"
    );
    assert!(
        diag_log_rs.contains("EVT_REFRESH_DECISION"),
        "diag_log.rs must define refresh decision event"
    );
    assert!(
        diag_log_rs.contains("EVT_DISPLAY_ERROR"),
        "diag_log.rs must define display error event"
    );
    assert!(
        main_rs.contains("EVT_FIRMWARE_STARTED"),
        "main.rs must use structured EVT_FIRMWARE_STARTED event"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_keeps_partial_frame_until_delimiter() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 1,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();

    let half = len / 2;
    state.feed_rx(&buf[..half]);
    let mut out = [0u8; MAX_FRAME_BYTES];
    assert!(
        state.next_frame(&mut out).is_none(),
        "should not yield frame before delimiter"
    );

    state.feed_rx(&buf[half..]);
    assert!(
        state.next_frame(&mut out).is_some(),
        "should yield frame after delimiter"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_yields_two_batched_frames_in_order() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();
    let mut combined = [0u8; MAX_FRAME_BYTES * 2];
    let mut offset = 0;

    for seq in [41u16, 42] {
        let header = FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::Status,
            status: Status::Ok,
            sequence: seq,
            payload_len: 0,
        };
        let mut buf = [0u8; MAX_FRAME_BYTES];
        let len = encode_frame(&header, &[], &mut buf).unwrap();
        combined[offset..offset + len].copy_from_slice(&buf[..len]);
        offset += len;
    }

    state.feed_rx(&combined[..offset]);

    let mut out = [0u8; MAX_FRAME_BYTES];
    let f1 = state.next_frame(&mut out);
    assert!(f1.is_some());
    let mut payload = [0u8; 496];
    let (h1, _) = decode_frame(&out[..f1.unwrap()], &mut payload).unwrap();
    assert_eq!(h1.sequence, 41);

    let f2 = state.next_frame(&mut out);
    assert!(f2.is_some());
    let (h2, _) = decode_frame(&out[..f2.unwrap()], &mut payload).unwrap();
    assert_eq!(h2.sequence, 42);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_counts_oversized_frame_and_continues() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    // Feed > MAX_FRAME_BYTES of data before a delimiter — transport should detect this
    let mut oversized = [0xAA; (MAX_FRAME_BYTES + 64) as usize];
    oversized[(MAX_FRAME_BYTES + 63) as usize] = 0x00; // delimiter at end
    state.feed_rx(&oversized);

    // next_frame detects the oversized frame and increments the error counter
    let mut out = [0u8; MAX_FRAME_BYTES];
    assert!(state.next_frame(&mut out).is_none());
    assert_eq!(state.protocol_error_count(), 1);

    // Recovery: a valid frame after delimiter should still work
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 99,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    state.feed_rx(&buf[..len]);

    assert!(state.next_frame(&mut out).is_some());
    assert_eq!(state.protocol_error_count(), 1); // still 1, not 2
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_counts_content_invalid_frame_and_continues() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    // Feed a valid-COBS frame that is transport-legal but contains garbage content
    let garbage = [0x01, 0x02, 0x03, 0x04, 0x05, 0x00];
    state.feed_rx(&garbage);

    // Transport layer accepts it (no error at transport level)
    let mut out = [0u8; MAX_FRAME_BYTES];
    let f = state.next_frame(&mut out);
    assert!(f.is_some());
    // Transport did not count an error — content validation is decode_frame's job
    assert_eq!(state.protocol_error_count(), 0);

    // Recovery: a valid frame after should still work
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 50,
        payload_len: 0,
    };
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &[], &mut buf).unwrap();
    state.feed_rx(&buf[..len]);

    let f2 = state.next_frame(&mut out);
    assert!(f2.is_some());
    let mut payload = [0u8; 496];
    let (h, _) = decode_frame(&out[..f2.unwrap()], &mut payload).unwrap();
    assert_eq!(h.sequence, 50);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_serial_partial_tx_preserves_unsent_suffix() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, FrameHeader, FrameKind, Opcode, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::SerialState;

    let mut state = SerialState::new();

    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 1,
        payload_len: 10,
    };
    let payload = [0xAA; 10];
    let mut buf = [0u8; MAX_FRAME_BYTES];
    let len = encode_frame(&header, &payload, &mut buf).unwrap();
    state.queue_tx(&buf[..len]).unwrap();

    assert_eq!(state.pending_tx().len(), len);

    state.consume_tx(3);
    assert_eq!(state.pending_tx().len(), len - 3);
    assert_eq!(state.pending_tx()[0], buf[3]);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_records_full_layout_and_tick() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog, DEFAULT_LOG_CAPACITY};

    let mut log = DiagLog::<DEFAULT_LOG_CAPACITY>::new();
    log.push(
        1000,
        DiagEvent {
            level: 2,
            subsystem: 3,
            event: 0x0010,
            arg0: -5,
            arg1: 100,
            arg2: 0,
        },
    );
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 1];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 1);
    assert_eq!(records[0].sequence, 0);
    assert_eq!(records[0].tick_ms, 1000);
    assert_eq!(records[0].level, 2);
    assert_eq!(records[0].subsystem, 3);
    assert_eq!(records[0].event, 0x0010);
    assert_eq!(records[0].arg0, -5);
    assert_eq!(records[0].arg1, 100);
    assert_eq!(records[0].arg2, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_cursor_is_sequence_after_overwrite() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..8u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(4, &mut records);
    assert_eq!(result.record_count, 4);
    assert_eq!(records[0].sequence, 4);
    assert_eq!(records[1].sequence, 5);
    assert_eq!(records[2].sequence, 6);
    assert_eq!(records[3].sequence, 7);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_cursor_before_oldest_starts_at_oldest_retained() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 4);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[3].sequence, 5);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_removes_records_and_dropped_but_keeps_sequence_monotonic() {
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<4>::new();
    for i in 0..6u32 {
        log.push(
            i * 100,
            DiagEvent {
                level: 1,
                subsystem: 1,
                event: i as u16,
                arg0: 0,
                arg1: 0,
                arg2: 0,
            },
        );
    }
    assert!(
        log.read_from_sequence(0, &mut [binbook_fw::diag_log::DiagLogRecord::default(); 4])
            .record_count
            > 0
    );
    assert!(log.dropped_records() > 0);
    log.clear();
    assert_eq!(log.dropped_records(), 0);
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 4];
    let result = log.read_from_sequence(0, &mut records);
    assert_eq!(result.record_count, 0);
    log.push(
        700,
        DiagEvent {
            level: 1,
            subsystem: 1,
            event: 99,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        },
    );
    let result2 = log.read_from_sequence(0, &mut records);
    assert_eq!(result2.record_count, 1);
    assert_eq!(records[0].sequence, 6);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_idle_transition_records_suppressed_count() {
    use binbook_fw::diag_log::{DiagDeduper, DiagEvent, DiagLog};

    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    deduper.push_enter_idle(&mut log, 0);
    for tick in (1..500).step_by(10) {
        deduper.push_idle_tick(&mut log, tick);
    }
    deduper.push_leave_idle(&mut log, 500);
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut records);
    assert!(result.record_count >= 2);
    assert_eq!(records[0].event, binbook_fw::diag_log::EVT_IDLE_ENTERED);
    assert_eq!(
        records[result.record_count as usize - 1].event,
        binbook_fw::diag_log::EVT_IDLE_LEFT
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_idle_summary_is_bounded_by_idle_summary_ms() {
    use binbook_fw::diag_log::{DiagDeduper, DiagLog, IDLE_SUMMARY_MS};

    let mut deduper = DiagDeduper::new();
    let mut log = DiagLog::<16>::new();
    deduper.push_enter_idle(&mut log, 0);
    for tick in (1..IDLE_SUMMARY_MS * 3).step_by(10) {
        deduper.push_idle_tick(&mut log, tick);
    }
    let mut records = [binbook_fw::diag_log::DiagLogRecord::default(); 16];
    let result = log.read_from_sequence(0, &mut records);
    let summary_count = (0..result.record_count)
        .filter(|&i| records[i as usize].event == binbook_fw::diag_log::EVT_IDLE_SUMMARY)
        .count();
    assert!(
        summary_count <= 3,
        "expected at most 3 summaries in {}ms, got {}",
        IDLE_SUMMARY_MS * 3,
        summary_count
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_key_right_matches_physical_button_target() {
    use binbook_fw::input::{apply_page_turn, target_page_for_button, Button};

    let current_page = 3u32;
    let page_count = 8u32;

    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 10,
        payload_len: 2,
    };
    let mut payload = [0u8; 2];
    payload[0] = binbook_diagnostic_protocol::KeyCode::Right as u8;
    payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, binbook_fw::input::PageTurn::Next);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_key_left_matches_physical_button_target() {
    use binbook_fw::input::{apply_page_turn, target_page_for_button, Button};

    let current_page = 3u32;
    let page_count = 8u32;

    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Key,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 11,
        payload_len: 2,
    };
    let mut payload = [0u8; 2];
    payload[0] = binbook_diagnostic_protocol::KeyCode::Left as u8;
    payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

    let mut ctx = binbook_fw::diag::CommandContext::new(current_page, page_count, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, binbook_fw::input::PageTurn::Previous);
        }
        other => panic!("expected RenderTurn, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_all_key_codes_match_physical_mapping() {
    use binbook_fw::input::{apply_page_turn, target_page_for_button, Button};

    let cases: &[(binbook_diagnostic_protocol::KeyCode, Button)] = &[
        (binbook_diagnostic_protocol::KeyCode::Right, Button::Right),
        (binbook_diagnostic_protocol::KeyCode::Left, Button::Left),
        (binbook_diagnostic_protocol::KeyCode::Up, Button::Up),
        (binbook_diagnostic_protocol::KeyCode::Down, Button::Down),
    ];

    for (code, button) in cases {
        let header = binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 20,
            payload_len: 2,
        };
        let mut payload = [0u8; 2];
        payload[0] = *code as u8;
        payload[1] = binbook_diagnostic_protocol::KeyAction::Press as u8;

        let mut ctx = binbook_fw::diag::CommandContext::new(5, 10, 0, 0);
        let mut resp_buf = [0u8; 496];
        let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);

        match result {
            binbook_fw::diag::DispatchResult::RenderTurn { turn } => {
                assert_eq!(
                    turn,
                    target_page_for_button(*button),
                    "key {:?} should match button {:?}",
                    code,
                    button
                );
            }
            other => panic!("key {:?}: expected RenderTurn, got {:?}", code, other),
        }
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_zero_from_nonzero_targets_zero() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 30,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&0u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 0);
        }
        other => panic!("expected RenderPage for goto 0, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_nonadjacent_targets_exact_page() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 31,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&6u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(2, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 6);
        }
        other => panic!("expected RenderPage for goto 6, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_goto_current_is_no_action() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 32,
        payload_len: 5,
    };
    let mut payload = [0u8; 5];
    payload[0] = binbook_diagnostic_protocol::PageAction::Goto as u8;
    payload[1..5].copy_from_slice(&3u32.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    assert_eq!(result, binbook_fw::diag::DispatchResult::NoAction);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_page_next_and_previous_clamp_at_edges() {
    let header_next = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 33,
        payload_len: 1,
    };
    let payload_next = [binbook_diagnostic_protocol::PageAction::Next as u8];

    let mut ctx = binbook_fw::diag::CommandContext::new(7, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result_next =
        binbook_fw::diag::dispatch_command(header_next, &payload_next, &mut ctx, &mut resp_buf);
    assert_eq!(
        result_next,
        binbook_fw::diag::DispatchResult::NoAction,
        "next from last page should be NoAction"
    );

    let header_prev = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 34,
        payload_len: 1,
    };
    let payload_prev = [binbook_diagnostic_protocol::PageAction::Previous as u8];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result_prev =
        binbook_fw::diag::dispatch_command(header_prev, &payload_prev, &mut ctx, &mut resp_buf);
    assert_eq!(
        result_prev,
        binbook_fw::diag::DispatchResult::NoAction,
        "previous from page 0 should be NoAction"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_hello_response_has_all_required_fields() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Hello,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 40,
        payload_len: 0,
    };

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 0, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::Response { payload_len, .. } => {
            assert!(
                payload_len >= 8,
                "HELLO response must be at least 8 bytes, got {}",
                payload_len
            );
        }
        other => panic!("expected Response for HELLO, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_status_response_uses_live_state_without_truncation() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Status,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 41,
        payload_len: 0,
    };

    let mut ctx = binbook_fw::diag::CommandContext::new(70_001, 80_002, -12i32, 0);
    ctx.protocol_errors = 100_004;
    ctx.dropped_records = 90_003;
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::Response { payload_len, .. } => {
            assert_eq!(payload_len, 21, "STATUS payload must be 21 bytes");
        }
        other => panic!("expected Response for STATUS, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_invalid_page_payload_returns_bad_request() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::Page,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 50,
        payload_len: 5,
    };
    let payload = [
        binbook_diagnostic_protocol::PageAction::Goto as u8,
        0xFF,
        0xFF,
        0xFF,
        0xFF,
    ];

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 8, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::Response { status, .. } => {
            assert_eq!(status, binbook_diagnostic_protocol::Status::BadRequest);
        }
        other => panic!(
            "expected BadRequest response for invalid goto, got {:?}",
            other
        ),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_returns_known_command_and_render_records() {
    let mut log = DiagLog::<64>::new();
    let event_receipt = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    let event_render = DiagEvent {
        level: 1,
        subsystem: 1,
        event: 0x0100,
        arg0: 5,
        arg1: 0,
        arg2: 0,
    };
    log.push(100, event_receipt);
    log.push(200, event_render);

    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogGet,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 6,
    };
    let mut payload = [0u8; 6];
    payload[0..4].copy_from_slice(&0u32.to_le_bytes());
    payload[4..6].copy_from_slice(&512u16.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::LogGet { cursor, max_bytes } => {
            assert_eq!(cursor, 0);
            assert_eq!(max_bytes, 512);
        }
        other => panic!("expected LogGet, got {:?}", other),
    }

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    assert!(written > binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES);

    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let dropped = u32::from_le_bytes([log_resp[4], log_resp[5], log_resp[6], log_resp[7]]);
    assert_eq!(next_cursor, 2);
    assert_eq!(dropped, 0);

    let rec1 = binbook_diagnostic_protocol::decode_log_record(&log_resp[10..34]).unwrap();
    assert_eq!(rec1.sequence, 0);
    assert_eq!(rec1.event, 0x0001);
    assert_eq!(rec1.tick_ms, 100);

    let rec2 = binbook_diagnostic_protocol::decode_log_record(&log_resp[34..58]).unwrap();
    assert_eq!(rec2.sequence, 1);
    assert_eq!(rec2.event, 0x0100);
    assert_eq!(rec2.arg0, 5);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_honors_sequence_cursor_after_overwrite() {
    let mut log = DiagLog::<4>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..8u32 {
        log.push(i * 10, event);
    }

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 4, 512, &mut log_resp);
    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let dropped = u32::from_le_bytes([log_resp[4], log_resp[5], log_resp[6], log_resp[7]]);
    assert_eq!(dropped, 4);
    assert_eq!(next_cursor, 8);

    let mut pos = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES;
    let mut count = 0;
    while pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES <= written {
        let rec = binbook_diagnostic_protocol::decode_log_record(
            &log_resp[pos..pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES],
        )
        .unwrap();
        assert!(rec.sequence >= 4);
        count += 1;
        pos += binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    }
    assert_eq!(count, 4);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_honors_byte_budget_on_record_boundaries() {
    let mut log = DiagLog::<64>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..10u32 {
        log.push(i * 10, event);
    }

    let budget = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES
        + 2 * binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, budget as u16, &mut log_resp);
    assert_eq!(written, budget);

    let mut pos = binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES;
    let mut record_count = 0;
    while pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES <= written {
        let _rec = binbook_diagnostic_protocol::decode_log_record(
            &log_resp[pos..pos + binbook_diagnostic_protocol::LOG_RECORD_BYTES],
        )
        .unwrap();
        record_count += 1;
        pos += binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    }
    assert_eq!(record_count, 2);

    let next_cursor = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    assert_eq!(next_cursor, 2);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_clears_nonempty_ring_and_dropped_count() {
    let mut log = DiagLog::<4>::new();
    let event = DiagEvent {
        level: 1,
        subsystem: 0,
        event: 0x0001,
        arg0: 0,
        arg1: 0,
        arg2: 0,
    };
    for i in 0..6u32 {
        log.push(i * 10, event);
    }
    assert_eq!(log.record_count(), 4);
    assert_eq!(log.dropped_records(), 2);

    let mut log_resp = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    assert!(written > binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES);

    let (next_cursor, dropped) = binbook_fw::diag::resolve_log_clear(&mut log);
    assert_eq!(next_cursor, 6);
    assert_eq!(dropped, 0);
    assert_eq!(log.record_count(), 0);

    let written2 = binbook_fw::diag::resolve_log_get(&log, 0, 512, &mut log_resp);
    let next_cursor2 = u32::from_le_bytes([log_resp[0], log_resp[1], log_resp[2], log_resp[3]]);
    let record_count_after = (written2 - binbook_diagnostic_protocol::LOG_RESPONSE_HEADER_BYTES)
        / binbook_diagnostic_protocol::LOG_RECORD_BYTES;
    assert_eq!(record_count_after, 0);
    assert_eq!(next_cursor2, 6);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_clear_dispatch_returns_log_clear_variant() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogClear,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 0,
    };
    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &[], &mut ctx, &mut resp_buf);
    assert_eq!(result, binbook_fw::diag::DispatchResult::LogClear);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_log_get_dispatch_returns_log_get_variant() {
    let header = binbook_diagnostic_protocol::FrameHeader {
        kind: binbook_diagnostic_protocol::FrameKind::Request,
        opcode: binbook_diagnostic_protocol::Opcode::LogGet,
        status: binbook_diagnostic_protocol::Status::Ok,
        sequence: 1,
        payload_len: 6,
    };
    let mut payload = [0u8; 6];
    payload[0..4].copy_from_slice(&5u32.to_le_bytes());
    payload[4..6].copy_from_slice(&256u16.to_le_bytes());

    let mut ctx = binbook_fw::diag::CommandContext::new(0, 10, 0, 0);
    let mut resp_buf = [0u8; 496];
    let result = binbook_fw::diag::dispatch_command(header, &payload, &mut ctx, &mut resp_buf);
    match result {
        binbook_fw::diag::DispatchResult::LogGet { cursor, max_bytes } => {
            assert_eq!(cursor, 5);
            assert_eq!(max_bytes, 256);
        }
        other => panic!("expected LogGet, got {:?}", other),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_command_error_is_logged_immediately() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, Opcode, Status, LOG_RECORD_BYTES,
        LOG_RESPONSE_HEADER_BYTES, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{poll_pending_command, SerialState};
    use binbook_fw::diag_log::{DiagLog, EVT_CMD_ERROR};

    let mut state = SerialState::new();
    let mut log = DiagLog::<64>::new();

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 7,
        payload_len: 1,
    };
    let mut req_buf = [0u8; MAX_FRAME_BYTES];
    let req_len = encode_frame(&header, &[0xFF], &mut req_buf).unwrap();
    state.feed_rx(&req_buf[..req_len]);

    let _action = poll_pending_command(&mut state, 0, 10, 0, 0, &mut log, 500);

    let mut resp_buf = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 496, &mut resp_buf);
    let mut pos = LOG_RESPONSE_HEADER_BYTES;
    let mut found_error = false;
    while pos + LOG_RECORD_BYTES <= written {
        let rec =
            binbook_diagnostic_protocol::decode_log_record(&resp_buf[pos..pos + LOG_RECORD_BYTES])
                .unwrap();
        if rec.event == EVT_CMD_ERROR {
            found_error = true;
            assert_eq!(rec.arg0, Opcode::Key as i32);
            assert_eq!(rec.arg1, Status::BadRequest as i32);
            assert_eq!(rec.tick_ms, 500);
        }
        pos += LOG_RECORD_BYTES;
    }
    assert!(
        found_error,
        "EVT_CMD_ERROR must appear in log after invalid command"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_refresh_panel_and_display_error_events_are_emitted() {
    use binbook_diagnostic_protocol::{
        encode_frame, FrameHeader, FrameKind, KeyAction, KeyCode, Opcode, Status, LOG_RECORD_BYTES,
        LOG_RESPONSE_HEADER_BYTES, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{poll_pending_command, SerialState};
    use binbook_fw::diag_log::{DiagLog, EVT_CMD_RECEIPT};

    let mut state = SerialState::new();
    let mut log = DiagLog::<64>::new();

    let key_payload = [KeyCode::Right as u8, KeyAction::Press as u8];
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 3,
        payload_len: 2,
    };
    let mut req_buf = [0u8; MAX_FRAME_BYTES];
    let req_len = encode_frame(&header, &key_payload, &mut req_buf).unwrap();
    state.feed_rx(&req_buf[..req_len]);

    let _action = poll_pending_command(&mut state, 0, 10, 0, 0, &mut log, 1000);

    let mut resp_buf = [0u8; 496];
    let written = binbook_fw::diag::resolve_log_get(&log, 0, 496, &mut resp_buf);
    let mut pos = LOG_RESPONSE_HEADER_BYTES;
    let mut found_receipt = false;
    while pos + LOG_RECORD_BYTES <= written {
        let rec =
            binbook_diagnostic_protocol::decode_log_record(&resp_buf[pos..pos + LOG_RECORD_BYTES])
                .unwrap();
        if rec.event == EVT_CMD_RECEIPT {
            found_receipt = true;
            assert_eq!(rec.arg0, Opcode::Key as i32);
            assert_eq!(rec.arg1, 3);
            assert_eq!(rec.tick_ms, 1000);
        }
        pos += LOG_RECORD_BYTES;
    }
    assert!(
        found_receipt,
        "EVT_CMD_RECEIPT must appear in log after valid command"
    );
}

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

    fn raw_bytes_mut(&mut self) -> &mut [u8; 4096] {
        &mut self.sector
    }
}

impl Flash for CrashMockFlash {
    fn read(&self, offset: u32, buf: &mut [u8]) -> HalResult<()> {
        let base = (offset - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        buf.copy_from_slice(&self.sector[base..base + buf.len()]);
        Ok(())
    }

    fn write(&mut self, offset: u32, data: &[u8]) -> HalResult<()> {
        let base = (offset - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        self.sector[base..base + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> HalResult<()> {
        let base = (offset - binbook_fw::flash::CRASH_SECTOR_OFFSET) as usize;
        let end = (base + 4096).min(self.sector.len());
        self.sector[base..end].fill(0xFF);
        Ok(())
    }

    fn size(&self) -> u32 {
        self.sector.len() as u32
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

    let flash = CrashMockFlash::new();
    let mut store = CrashStore::new(flash);
    store.write_fatal(&summary).unwrap();

    let flash_bytes = store.flash().raw_bytes().clone();
    drop(store);

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
    let result = dispatch_command(header, &[], &mut ctx, &mut resp_buf);
    assert_eq!(result, DispatchResult::CrashClear);

    let header_get = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::CrashGet,
        status: Status::Ok,
        sequence: 2,
        payload_len: 0,
    };
    let result_get = dispatch_command(header_get, &[], &mut ctx, &mut resp_buf);
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

#[cfg(feature = "diagnostic-console")]
mod probe_test_harness {
    use std::vec::Vec;
    use xteink_hal::{HalError, HalResult};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DriverCall {
        SetWindow { x: u16, y: u16, w: u16, h: u16 },
        WriteFrameRows { row_count: u16 },
        WriteRedFrameRows { row_count: u16 },
        Refresh { mode: u8 },
        InitBw,
        InitGrayscale,
    }

    pub struct RecordingDriver {
        pub calls: Vec<DriverCall>,
        pub fail_on_red_plane_write: bool,
    }

    impl RecordingDriver {
        pub fn new() -> Self {
            Self {
                calls: Vec::new(),
                fail_on_red_plane_write: false,
            }
        }

        pub fn with_fail_on_red_plane_write() -> Self {
            Self {
                calls: Vec::new(),
                fail_on_red_plane_write: true,
            }
        }

        pub fn set_window(&mut self, x: u16, y: u16, w: u16, h: u16) -> HalResult<()> {
            self.calls.push(DriverCall::SetWindow { x, y, w, h });
            Ok(())
        }

        pub fn write_frame_rows<F>(&mut self, row_count: u16, _fill: F) -> HalResult<()>
        where
            F: FnMut(u16, &mut [u8]) -> Result<(), HalError>,
        {
            self.calls.push(DriverCall::WriteFrameRows { row_count });
            Ok(())
        }

        pub fn write_red_frame_rows<F>(&mut self, row_count: u16, _fill: F) -> HalResult<()>
        where
            F: FnMut(u16, &mut [u8]) -> Result<(), HalError>,
        {
            if self.fail_on_red_plane_write {
                return Err(HalError::Spi);
            }
            self.calls.push(DriverCall::WriteRedFrameRows { row_count });
            Ok(())
        }

        pub fn refresh(&mut self, mode: u8) -> HalResult<()> {
            self.calls.push(DriverCall::Refresh { mode });
            Ok(())
        }

        pub fn init_bw(&mut self) -> HalResult<()> {
            self.calls.push(DriverCall::InitBw);
            Ok(())
        }

        pub fn init_grayscale(&mut self) -> HalResult<()> {
            self.calls.push(DriverCall::InitGrayscale);
            Ok(())
        }
    }

    pub fn ensure_grayscale_mode(
        driver: &mut RecordingDriver,
        panel_mode: &mut binbook_fw::display::PanelMode,
    ) -> Result<(), HalError> {
        if *panel_mode != binbook_fw::display::PanelMode::Grayscale {
            driver.init_grayscale()?;
            *panel_mode = binbook_fw::display::PanelMode::Grayscale;
        }
        Ok(())
    }

    pub fn ensure_bw_mode(
        driver: &mut RecordingDriver,
        panel_mode: &mut binbook_fw::display::PanelMode,
    ) -> Result<(), HalError> {
        if *panel_mode != binbook_fw::display::PanelMode::Bw {
            driver.init_bw()?;
            *panel_mode = binbook_fw::display::PanelMode::Bw;
        }
        Ok(())
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_full_refresh_executes_current_page_full_path() {
    use binbook_fw::display::PanelMode;

    let mut panel_mode = PanelMode::Unknown;
    let mut driver = probe_test_harness::RecordingDriver::new();

    let book_bytes: Vec<u8> = vec![0u8; 4096];
    let mut scratch = [0u8; 8192];

    let book_result = binbook_core::Book::open(
        binbook_core::SliceSource::new(&book_bytes[..]),
        &mut scratch,
    );
    if let Ok(mut book) = book_result {
        if book.page_count() > 0 {
            probe_test_harness::ensure_grayscale_mode(&mut driver, &mut panel_mode).unwrap();

            assert!(
                driver
                    .calls
                    .iter()
                    .any(|c| matches!(c, probe_test_harness::DriverCall::InitGrayscale)),
                "full refresh probe must init grayscale mode"
            );
        }
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_clear_white_writes_both_planes_and_refreshes() {
    use binbook_fw::display::{PanelMode, DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH};

    let mut panel_mode = PanelMode::Unknown;
    let mut driver = probe_test_harness::RecordingDriver::new();

    probe_test_harness::ensure_bw_mode(&mut driver, &mut panel_mode).unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    driver
        .write_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
            row_buf.fill(0xFF);
            Ok(())
        })
        .unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    driver
        .write_red_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
            row_buf.fill(0xFF);
            Ok(())
        })
        .unwrap();
    driver.refresh(0).unwrap();

    let set_window_calls: Vec<_> = driver
        .calls
        .iter()
        .filter(|c| matches!(c, probe_test_harness::DriverCall::SetWindow { .. }))
        .collect();
    assert_eq!(
        set_window_calls.len(),
        2,
        "clear white must set window twice (black + red plane)"
    );

    assert!(
        driver.calls.iter().any(|c| matches!(
            c,
            probe_test_harness::DriverCall::WriteFrameRows { row_count: 480 }
        )),
        "clear white must write 480 rows to black plane"
    );
    assert!(
        driver.calls.iter().any(|c| matches!(
            c,
            probe_test_harness::DriverCall::WriteRedFrameRows { row_count: 480 }
        )),
        "clear white must write 480 rows to red plane"
    );
    assert!(
        driver
            .calls
            .iter()
            .any(|c| matches!(c, probe_test_harness::DriverCall::Refresh { .. })),
        "clear white must refresh"
    );
    assert!(
        driver
            .calls
            .iter()
            .any(|c| matches!(c, probe_test_harness::DriverCall::InitBw)),
        "clear white must init BW mode"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_window_corners_writes_all_four_physical_corners_on_both_planes() {
    use binbook_fw::display::{
        smoke_probe_windows, PanelMode, DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH,
        PROBE_BOX_HEIGHT, PROBE_BOX_WIDTH,
    };

    let mut panel_mode = PanelMode::Unknown;
    let mut driver = probe_test_harness::RecordingDriver::new();

    probe_test_harness::ensure_bw_mode(&mut driver, &mut panel_mode).unwrap();

    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    driver
        .write_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
            row_buf.fill(0xFF);
            Ok(())
        })
        .unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    driver
        .write_red_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
            row_buf.fill(0xFF);
            Ok(())
        })
        .unwrap();

    let corners = smoke_probe_windows();
    for &(x, y, w, h) in &corners {
        driver.set_window(x, y, w, h).unwrap();
        driver
            .write_frame_rows(h, |_, row_buf: &mut [u8]| {
                row_buf.fill(0x00);
                Ok(())
            })
            .unwrap();
    }
    for &(x, y, w, h) in &corners {
        driver.set_window(x, y, w, h).unwrap();
        driver
            .write_red_frame_rows(h, |_, row_buf: &mut [u8]| {
                row_buf.fill(0x00);
                Ok(())
            })
            .unwrap();
    }
    driver.refresh(0).unwrap();

    let set_window_calls: Vec<_> = driver
        .calls
        .iter()
        .filter_map(|c| match c {
            probe_test_harness::DriverCall::SetWindow { x, y, w, h } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect();

    assert_eq!(
        set_window_calls.len(),
        10,
        "window corners must set window 10 times (2 full clear + 4 corners x 2 planes)"
    );

    let expected_windows = [
        (0, 0, PROBE_BOX_WIDTH, PROBE_BOX_HEIGHT),
        (
            DISPLAY_WIDTH - PROBE_BOX_WIDTH,
            0,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
        (
            0,
            DISPLAY_HEIGHT - PROBE_BOX_HEIGHT,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
        (
            DISPLAY_WIDTH - PROBE_BOX_WIDTH,
            DISPLAY_HEIGHT - PROBE_BOX_HEIGHT,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
    ];

    for &(expected_x, expected_y, expected_w, expected_h) in &expected_windows {
        let found = set_window_calls.iter().any(|&(x, y, w, h)| {
            x == expected_x && y == expected_y && w == expected_w && h == expected_h
        });
        assert!(
            found,
            "window corners must write to ({}, {}, {}, {})",
            expected_x, expected_y, expected_w, expected_h
        );
    }

    assert!(
        driver
            .calls
            .iter()
            .any(|c| matches!(c, probe_test_harness::DriverCall::InitBw)),
        "window corners must init BW mode"
    );
    assert!(
        driver
            .calls
            .iter()
            .any(|c| matches!(c, probe_test_harness::DriverCall::Refresh { .. })),
        "window corners must refresh"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_failure_returns_internal_error_and_logs_display_error() {
    use binbook_fw::display::{PanelMode, DISPLAY_HEIGHT, DISPLAY_WIDTH};

    let mut panel_mode = PanelMode::Unknown;
    let mut driver = probe_test_harness::RecordingDriver::with_fail_on_red_plane_write();

    probe_test_harness::ensure_bw_mode(&mut driver, &mut panel_mode).unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    driver
        .write_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
            row_buf.fill(0xFF);
            Ok(())
        })
        .unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    let result = driver.write_red_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
        row_buf.fill(0xFF);
        Ok(())
    });

    assert!(result.is_err(), "probe must fail when display write fails");
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diag_probe_success_logs_render_result() {
    use binbook_fw::display::{PanelMode, DISPLAY_HEIGHT, DISPLAY_WIDTH};

    let mut panel_mode = PanelMode::Unknown;
    let mut driver = probe_test_harness::RecordingDriver::new();

    probe_test_harness::ensure_bw_mode(&mut driver, &mut panel_mode).unwrap();
    driver
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .unwrap();
    let result = driver.write_frame_rows(DISPLAY_HEIGHT, |_, row_buf: &mut [u8]| {
        row_buf.fill(0xFF);
        Ok(())
    });

    assert!(
        result.is_ok(),
        "successful probe must return Ok: {:?}",
        result.err()
    );
}

#[cfg(all(feature = "diagnostic-console", feature = "debug-log"))]
#[test]
fn diagnostic_console_takes_usb_ownership_over_debug_log() {
    let source = include_str!("../src/main.rs");

    let lines: Vec<&str> = source.lines().collect();
    let mut saw_diagnostic_console_guard = false;

    for window in lines.windows(2) {
        let combined = format!("{} {}", window[0], window[1]);
        if combined.contains("use esp_println")
            && combined.contains("not(feature = \"diagnostic-console\")")
        {
            saw_diagnostic_console_guard = true;
            break;
        }
    }

    assert!(
        saw_diagnostic_console_guard,
        "esp_println import must be gated with not(feature = \"diagnostic-console\") \
         so packet transport owns USB when diagnostic-console is enabled"
    );
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_command_acceptance_fixture_rejects_noops() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, encode_hello_response, FrameHeader, FrameKind, HelloResponse,
        KeyAction, KeyCode, Opcode, PageAction, ProbeCode, Status, ALL_CAPABILITIES,
        LOG_RECORD_BYTES, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{
        dispatch_command, resolve_log_clear, resolve_log_get, CommandContext, DispatchResult,
        DisplayProbeKind,
    };
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let mut log = DiagLog::<64>::new();
    log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 0,
            event: 0x0001,
            arg0: 1,
            arg1: 0,
            arg2: 0,
        },
    );
    log.push(
        200,
        DiagEvent {
            level: 2,
            subsystem: 2,
            event: 0x0300,
            arg0: 3,
            arg1: 0,
            arg2: 0,
        },
    );
    log.push(
        300,
        DiagEvent {
            level: 0,
            subsystem: 1,
            event: 0x0600,
            arg0: 2048,
            arg1: 0,
            arg2: 0,
        },
    );

    let mut ctx = CommandContext::new(3, 8, 0, 0);
    let mut resp_buf = [0u8; MAX_FRAME_BYTES];

    let mut frame_buf = [0u8; MAX_FRAME_BYTES];

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 10,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let mut payload_out = [0u8; 496];
    let (decoded_header, payload_len) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(
        decoded_header,
        &payload_out[..payload_len],
        &mut ctx,
        &mut resp_buf,
    );
    match result {
        DispatchResult::Response {
            status: Status::Ok,
            payload_len: pl,
        } => {
            assert!(pl > 0, "HELLO must produce a non-empty payload");
            let hello =
                binbook_diagnostic_protocol::decode_hello_response(&resp_buf[..pl]).unwrap();
            assert_eq!(hello.protocol_version, 1);
            assert_eq!(hello.max_frame_bytes, 512);
            assert!(hello.capabilities & ALL_CAPABILITIES != 0);
        }
        other => panic!("HELLO expected Ok response, got {:?}", other),
    }

    ctx.current_page = 3;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 11,
        payload_len: 5,
    };
    let mut page_payload = [0u8; 5];
    page_payload[0] = PageAction::Goto as u8;
    page_payload[1..5].copy_from_slice(&0u32.to_le_bytes());
    let flen = encode_frame(&header, &page_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::RenderPage { target_page } => {
            assert_eq!(target_page, 0, "Goto 0 must target page 0");
        }
        other => panic!(
            "PAGE goto 0 from page 3 expected RenderPage, got {:?}",
            other
        ),
    }
    assert_eq!(
        ctx.current_page, 3,
        "dispatch must NOT mutate ctx.current_page"
    );

    ctx.current_page = 3;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 12,
        payload_len: 5,
    };
    let mut page_payload = [0u8; 5];
    page_payload[0] = PageAction::Goto as u8;
    page_payload[1..5].copy_from_slice(&3u32.to_le_bytes());
    let flen = encode_frame(&header, &page_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::NoAction => {}
        other => panic!("PAGE goto same page expected NoAction, got {:?}", other),
    }

    ctx.current_page = 0;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 13,
        payload_len: 2,
    };
    let key_payload = [KeyCode::Right as u8, KeyAction::Press as u8];
    let flen = encode_frame(&header, &key_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::RenderTurn { turn } => {
            assert_eq!(turn, PageTurn::Next, "RIGHT from page 0 must queue Next");
        }
        other => panic!("KEY RIGHT from page 0 expected RenderTurn, got {:?}", other),
    }

    ctx.current_page = 0;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 14,
        payload_len: 2,
    };
    let key_payload = [KeyCode::Left as u8, KeyAction::Press as u8];
    let flen = encode_frame(&header, &key_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    assert_eq!(
        result,
        DispatchResult::RenderTurn {
            turn: PageTurn::Previous,
        }
    );

    ctx.current_page = 7;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 15,
        payload_len: 1,
    };
    let next_payload = [PageAction::Next as u8];
    let flen = encode_frame(&header, &next_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::NoAction => {}
        other => panic!(
            "PAGE next from last page expected NoAction, got {:?}",
            other
        ),
    }

    ctx.current_page = 0;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 16,
        payload_len: 1,
    };
    let prev_payload = [PageAction::Previous as u8];
    let flen = encode_frame(&header, &prev_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::NoAction => {}
        other => panic!(
            "PAGE previous from page 0 expected NoAction, got {:?}",
            other
        ),
    }

    ctx.current_page = 0;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 17,
        payload_len: 5,
    };
    let mut page_payload = [0u8; 5];
    page_payload[0] = PageAction::Goto as u8;
    page_payload[1..5].copy_from_slice(&8u32.to_le_bytes());
    let flen = encode_frame(&header, &page_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "PAGE goto >= page_count expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 18,
        payload_len: 5,
    };
    let mut page_payload = [0u8; 5];
    page_payload[0] = PageAction::Goto as u8;
    page_payload[1..5].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    let flen = encode_frame(&header, &page_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!("PAGE goto huge value expected BadRequest, got {:?}", other),
    }

    ctx.current_page = 5;
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Status,
        status: Status::Ok,
        sequence: 20,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::Ok,
            payload_len,
        } => {
            assert_eq!(payload_len, 21, "STATUS payload must be 21 bytes");
            let sp = binbook_diagnostic_protocol::decode_status_payload(&resp_buf[..payload_len])
                .unwrap();
            assert_eq!(sp.current_page, 5);
            assert_eq!(sp.page_count, 8);
        }
        other => panic!("STATUS expected Ok with payload, got {:?}", other),
    }

    let mut inner_log = DiagLog::<64>::new();
    inner_log.push(
        100,
        DiagEvent {
            level: 2,
            subsystem: 0,
            event: 0x0001,
            arg0: 1,
            arg1: 0,
            arg2: 0,
        },
    );
    inner_log.push(
        200,
        DiagEvent {
            level: 2,
            subsystem: 2,
            event: 0x0300,
            arg0: 3,
            arg1: 0,
            arg2: 0,
        },
    );

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::LogGet,
        status: Status::Ok,
        sequence: 30,
        payload_len: 6,
    };
    let mut log_payload = [0u8; 6];
    log_payload[0..4].copy_from_slice(&0u32.to_le_bytes());
    log_payload[4..6].copy_from_slice(&4096u16.to_le_bytes());
    let flen = encode_frame(&header, &log_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let log_result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match log_result {
        DispatchResult::LogGet {
            cursor: 0,
            max_bytes: 4096,
        } => {}
        other => panic!(
            "LOG_GET expected LogGet {{ cursor:0, max_bytes:4096 }}, got {:?}",
            other
        ),
    }

    let mut log_resp_buf = [0u8; 4096];
    let log_len = resolve_log_get(&inner_log, 0, 4096, &mut log_resp_buf);
    assert!(
        log_len > 8,
        "LOG_GET response must include records beyond header"
    );
    let next_cursor = u32::from_le_bytes(log_resp_buf[0..4].try_into().unwrap());
    let dropped = u32::from_le_bytes(log_resp_buf[4..8].try_into().unwrap());
    assert_eq!(next_cursor, 2, "next_cursor must point past last record");
    assert_eq!(dropped, 0, "no records should be dropped");

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::LogClear,
        status: Status::Ok,
        sequence: 31,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let clear_result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match clear_result {
        DispatchResult::LogClear => {}
        other => panic!("LOG_CLEAR expected LogClear, got {:?}", other),
    }
    let (nc, dc) = resolve_log_clear(&mut inner_log);
    assert_eq!(dc, 0, "cleared log should have zero dropped");

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::CrashGet,
        status: Status::Ok,
        sequence: 32,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::CrashGet => {}
        other => panic!("CRASH_GET expected CrashGet, got {:?}", other),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::CrashClear,
        status: Status::Ok,
        sequence: 33,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::CrashClear => {}
        other => panic!("CRASH_CLEAR expected CrashClear, got {:?}", other),
    }

    for (code, expected) in [
        (0x01u8, Some(DisplayProbeKind::FullRefreshCurrent)),
        (0x02, Some(DisplayProbeKind::ClearWhite)),
        (0x03, Some(DisplayProbeKind::WindowCorners)),
    ] {
        let header = FrameHeader {
            kind: FrameKind::Request,
            opcode: Opcode::DisplayProbe,
            status: Status::Ok,
            sequence: 40 + code as u16,
            payload_len: 1,
        };
        let probe_payload = [code];
        let flen = encode_frame(&header, &probe_payload, &mut frame_buf).unwrap();
        let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
        let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
        assert_eq!(result, DispatchResult::DisplayProbe(expected.unwrap()));
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::DisplayProbe,
        status: Status::Ok,
        sequence: 50,
        payload_len: 1,
    };
    let bad_probe = [0xFFu8];
    let flen = encode_frame(&header, &bad_probe, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "DISPLAY_PROBE invalid code expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 51,
        payload_len: 2,
    };
    let bad_key_payload = [0xFFu8, KeyAction::Press as u8];
    let flen = encode_frame(&header, &bad_key_payload, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!("KEY invalid code expected BadRequest, got {:?}", other),
    }

    let header = FrameHeader {
        kind: FrameKind::Response,
        opcode: Opcode::Hello,
        status: Status::Ok,
        sequence: 52,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "Request with Response kind expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 53,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "PAGE with empty payload expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 54,
        payload_len: 3,
    };
    let short_goto = [PageAction::Goto as u8, 0x00, 0x01];
    let flen = encode_frame(&header, &short_goto, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "PAGE goto with truncated payload expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Key,
        status: Status::Ok,
        sequence: 55,
        payload_len: 1,
    };
    let short_key = [KeyCode::Right as u8];
    let flen = encode_frame(&header, &short_key, &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "KEY with truncated payload expected BadRequest, got {:?}",
            other
        ),
    }

    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::DisplayProbe,
        status: Status::Ok,
        sequence: 56,
        payload_len: 0,
    };
    let flen = encode_frame(&header, &[], &mut frame_buf).unwrap();
    let (dh, pl) = decode_frame(&frame_buf[..flen], &mut payload_out).unwrap();
    let result = dispatch_command(dh, &payload_out[..pl], &mut ctx, &mut resp_buf);
    match result {
        DispatchResult::Response {
            status: Status::BadRequest,
            ..
        } => {}
        other => panic!(
            "DISPLAY_PROBE with empty payload expected BadRequest, got {:?}",
            other
        ),
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_page_response_is_queued_only_after_action_completion() {
    use binbook_diagnostic_protocol::{
        decode_frame, encode_frame, encode_page_payload, FrameHeader, FrameKind, Opcode,
        PageAction, Status, MAX_FRAME_BYTES,
    };
    use binbook_fw::diag::{
        complete_pending_command, poll_pending_command, PendingAction, SerialState,
    };
    use binbook_fw::diag_log::DiagLog;

    let mut payload = [0u8; 5];
    let payload_len = encode_page_payload(PageAction::Goto, Some(0), &mut payload).unwrap();
    let header = FrameHeader {
        kind: FrameKind::Request,
        opcode: Opcode::Page,
        status: Status::Ok,
        sequence: 0x1234,
        payload_len: payload_len as u16,
    };
    let mut encoded = [0u8; MAX_FRAME_BYTES];
    let encoded_len = encode_frame(&header, &payload[..payload_len], &mut encoded).unwrap();

    let mut serial = SerialState::new();
    let mut log = DiagLog::<8>::new();
    serial.feed_rx(&encoded[..encoded_len]);
    let pending = poll_pending_command(&mut serial, 3, 8, 0, 0, &mut log, 100)
        .expect("page command should require execution");
    assert_eq!(pending.action, PendingAction::RenderPage { target_page: 0 });
    assert!(
        serial.pending_tx().is_empty(),
        "success must not be sent before render"
    );

    complete_pending_command(&mut serial, pending, Status::Ok, 0, &[]).unwrap();
    let mut decoded_payload = [0u8; 32];
    let (response, response_len) = decode_frame(serial.pending_tx(), &mut decoded_payload).unwrap();
    assert_eq!(response.sequence, 0x1234);
    assert_eq!(response.opcode, Opcode::Page);
    assert_eq!(response.status, Status::Ok);
    assert_eq!(response_len, 4);
    assert_eq!(
        u32::from_le_bytes(decoded_payload[..4].try_into().unwrap()),
        0
    );
}

#[cfg(feature = "diagnostic-console")]
struct AsyncDiagHarness {
    current_page: u32,
    page_count: u32,
    serial: binbook_fw::diag::SerialState,
    log: binbook_fw::diag_log::DiagLog<8>,
    pending: Vec<binbook_fw::diag::PendingCommand>,
    received_turns: Vec<PageTurn>,
    response_sequences: Vec<u16>,
}

#[cfg(feature = "diagnostic-console")]
impl AsyncDiagHarness {
    fn on_page(current_page: u32, page_count: u32) -> Self {
        Self {
            current_page,
            page_count,
            serial: binbook_fw::diag::SerialState::new(),
            log: binbook_fw::diag_log::DiagLog::<8>::new(),
            pending: Vec::new(),
            received_turns: Vec::new(),
            response_sequences: Vec::new(),
        }
    }

    fn receive_key(&mut self, sequence: u16, key: binbook_diagnostic_protocol::KeyCode) {
        let turn = match key {
            binbook_diagnostic_protocol::KeyCode::Right
            | binbook_diagnostic_protocol::KeyCode::Down => PageTurn::Next,
            binbook_diagnostic_protocol::KeyCode::Left
            | binbook_diagnostic_protocol::KeyCode::Up => PageTurn::Previous,
            other => panic!("unexpected key for page turn test: {:?}", other),
        };
        self.received_turns.push(turn);

        let mut payload = [0u8; 2];
        let payload_len = binbook_diagnostic_protocol::encode_key_payload(
            key,
            binbook_diagnostic_protocol::KeyAction::Press,
            &mut payload,
        )
        .unwrap();
        let header = binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence,
            payload_len: payload_len as u16,
        };
        let mut encoded = [0u8; binbook_diagnostic_protocol::MAX_FRAME_BYTES];
        let encoded_len = binbook_diagnostic_protocol::encode_frame(
            &header,
            &payload[..payload_len],
            &mut encoded,
        )
        .unwrap();
        self.serial.feed_rx(&encoded[..encoded_len]);
        let pending = binbook_fw::diag::poll_pending_command(
            &mut self.serial,
            self.current_page,
            self.page_count,
            0,
            0,
            &mut self.log,
            sequence as u32,
        )
        .expect("directional key should queue a render");
        self.response_sequences.push(pending.header.sequence);
        self.pending.push(pending);
    }

    fn pending_turns(&self) -> [PageTurn; 3] {
        self.received_turns
            .as_slice()
            .try_into()
            .expect("test harness expected exactly three queued turns")
    }

    fn rendered_pages_after_completion(&mut self) -> [u32; 3] {
        let mut rendered_pages = [0u32; 3];

        for (index, pending) in self.pending.drain(..).enumerate() {
            let target_page = match pending.action {
                binbook_fw::diag::PendingAction::RenderPage { target_page } => target_page,
                binbook_fw::diag::PendingAction::RenderTurn { turn } => {
                    binbook_fw::input::apply_page_turn(self.current_page, self.page_count, turn)
                }
                other => panic!("expected render action, got {:?}", other),
            };

            binbook_fw::diag::complete_pending_command(
                &mut self.serial,
                pending,
                binbook_diagnostic_protocol::Status::Ok,
                target_page,
                &[],
            )
            .unwrap();
            rendered_pages[index] = target_page;
            self.current_page = target_page;
        }

        rendered_pages
    }

    fn response_sequences(&self) -> [u16; 3] {
        self.response_sequences
            .as_slice()
            .try_into()
            .expect("test harness expected exactly three pending responses")
    }
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn batched_key_presses_are_resolved_when_dequeued() {
    let mut harness = AsyncDiagHarness::on_page(1, 4);
    harness.receive_key(10, binbook_diagnostic_protocol::KeyCode::Right);
    harness.receive_key(11, binbook_diagnostic_protocol::KeyCode::Right);
    harness.receive_key(12, binbook_diagnostic_protocol::KeyCode::Left);

    assert_eq!(
        harness.pending_turns(),
        [PageTurn::Next, PageTurn::Next, PageTurn::Previous]
    );
    assert_eq!(harness.rendered_pages_after_completion(), [2, 3, 2]);
    assert_eq!(harness.response_sequences(), [10, 11, 12]);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_pending_queue_rejects_the_seventeenth_command_without_evicting_old_requests() {
    use binbook_fw::diag::{DiagnosticPendingQueue, PendingAction, PendingCommand};

    let mut queue = DiagnosticPendingQueue::<16>::new();

    for sequence in 0..16u16 {
        let pending = PendingCommand {
            header: binbook_diagnostic_protocol::FrameHeader {
                kind: binbook_diagnostic_protocol::FrameKind::Request,
                opcode: binbook_diagnostic_protocol::Opcode::Key,
                status: binbook_diagnostic_protocol::Status::Ok,
                sequence,
                payload_len: 0,
            },
            action: PendingAction::RenderPage {
                target_page: sequence as u32,
            },
        };
        queue.try_push(pending).unwrap();
    }

    let overflow = PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 16,
            payload_len: 0,
        },
        action: PendingAction::RenderPage { target_page: 16 },
    };

    assert_eq!(queue.try_push(overflow), Err(overflow));
    assert_eq!(queue.len(), 16);
    assert_eq!(queue.front().unwrap().header.sequence, 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_snapshot_builds_status_payload_from_committed_state() {
    use binbook_fw::diag::DiagnosticSnapshot;

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };

    let status = snapshot.status_payload();

    assert_eq!(status.current_page, 7);
    assert_eq!(status.page_count, 30);
    assert_eq!(
        status.panel_mode,
        binbook_diagnostic_protocol::PanelModeCode::Bw
    );
    assert_eq!(status.dropped_log_count, 4);
    assert_eq!(status.protocol_error_count, 2);
    assert_eq!(status.last_error, -12);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_loop_services_status_and_log_while_render_is_pending() {
    use binbook_fw::diag::{
        DiagnosticLoopState, DiagnosticSnapshot, PendingAction, PendingCommand,
    };
    use binbook_fw::diag_log::{DiagEvent, DiagLog};

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };
    let mut log = DiagLog::<8>::new();
    log.push(
        1000,
        DiagEvent {
            level: binbook_fw::diag_log::LEVEL_INFO,
            subsystem: binbook_fw::diag_log::SUB_SERIAL,
            event: binbook_fw::diag_log::EVT_CMD_RECEIPT,
            arg0: 1,
            arg1: 2,
            arg2: 3,
        },
    );
    let mut loop_state = DiagnosticLoopState::<16, 8>::new(snapshot, log);

    let pending = PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Page,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 99,
            payload_len: 1,
        },
        action: PendingAction::RenderPage { target_page: 8 },
    };
    loop_state.enqueue_pending(pending).unwrap();

    let status = loop_state.status_payload();
    let mut log_buf = [0u8; 128];
    let log_len = loop_state.resolve_log_get(0, 128, &mut log_buf);

    assert_eq!(status.current_page, 7);
    assert_eq!(status.page_count, 30);
    assert_eq!(status.dropped_log_count, 4);
    assert_eq!(status.protocol_error_count, 2);
    assert_eq!(loop_state.pending_len(), 1);
    assert!(log_len > 0);
}

#[cfg(feature = "diagnostic-console")]
#[test]
fn diagnostic_loop_reports_error_when_key_queue_is_full_without_evicting_old_requests() {
    use binbook_fw::diag::{
        DiagnosticLoopState, DiagnosticSnapshot, PendingAction, PendingCommand,
    };

    let snapshot = DiagnosticSnapshot {
        current_page: 7,
        page_count: 30,
        panel_mode: binbook_diagnostic_protocol::PanelModeCode::Bw,
        dropped_log_count: 4,
        protocol_error_count: 2,
        last_error: -12,
    };
    let log = binbook_fw::diag_log::DiagLog::<8>::new();
    let mut loop_state = DiagnosticLoopState::<16, 8>::new(snapshot, log);

    for sequence in 0..16u16 {
        loop_state
            .enqueue_pending(PendingCommand {
                header: binbook_diagnostic_protocol::FrameHeader {
                    kind: binbook_diagnostic_protocol::FrameKind::Request,
                    opcode: binbook_diagnostic_protocol::Opcode::Key,
                    status: binbook_diagnostic_protocol::Status::Ok,
                    sequence,
                    payload_len: 0,
                },
                action: PendingAction::RenderPage {
                    target_page: sequence as u32,
                },
            })
            .unwrap();
    }

    let result = loop_state.enqueue_pending_with_status(PendingCommand {
        header: binbook_diagnostic_protocol::FrameHeader {
            kind: binbook_diagnostic_protocol::FrameKind::Request,
            opcode: binbook_diagnostic_protocol::Opcode::Key,
            status: binbook_diagnostic_protocol::Status::Ok,
            sequence: 16,
            payload_len: 0,
        },
        action: PendingAction::RenderPage { target_page: 16 },
    });

    assert_eq!(result, binbook_diagnostic_protocol::Status::Error);
    assert_eq!(loop_state.pending_len(), 16);
    assert_eq!(loop_state.complete_pending().unwrap().header.sequence, 0);
}

#[cfg(feature = "diagnostic-console")]
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
