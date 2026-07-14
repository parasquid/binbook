use binbook_core::{CompressionMethod, SliceSource};
use core::convert::Infallible;
use embedded_hal::{
    digital::{ErrorType as DigitalErrorType, InputPin, OutputPin},
    spi::{ErrorType as SpiErrorType, Operation, SpiDevice},
};
use std::{cell::RefCell, rc::Rc};
use xteink_x4_display::{
    buffers::RenderBuffers,
    events::OperationOutcome,
    framebuffer::{Gray2Color, Gray2Framebuffer},
    page_source::{PlaneDecodeStep, PlaneDecoder, StreamingPlaneDecoder},
    panel::{PanelRamTarget, X4Panel},
    render::{
        cleanup_fast_preview, render_absolute_gray, render_bw_differential, render_fast_preview,
        render_staged_overlay, sync_bw_base, OverlayControl,
    },
    stream::decode_stream,
    ui_render::{
        activate_clean, activate_differential, activate_fast_differential, decode_book_fast_base,
        render_book_to_ui_differential, render_ui_to_book_differential, render_ui_transition,
        render_ui_transition_fast, stage_book_fast_base, stage_decoded_fast_base, stage_ui_base,
        DifferentialRamTarget, FAST_BASE_BYTES,
    },
};

#[test]
fn clean_activation_uses_the_x4_single_pass_clean_sequence() {
    let (_, mut panel, trace) = fixture();
    let mut delay = Delay::default();

    block_on(activate_clean(&mut panel, &mut delay)).unwrap();

    let writes = trace.0.borrow();
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::TEMP_SENSOR_WRITE]
            && window[1].as_slice() == [ssd1677_driver::Command::CLEAN_REFRESH_TEMPERATURE]
    }));
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::DISPLAY_UPDATE_CTRL2]
            && window[1].as_slice() == [ssd1677_driver::Command::UPDATE_CTRL_CLEAN]
    }));
}

#[test]
fn staged_menu_book_planes_use_accepted_preview_and_cleanup_waveforms() {
    let (_, mut panel, trace) = fixture();
    let mut delay = Delay::default();

    block_on(activate_fast_differential(&mut panel, &mut delay)).unwrap();

    let writes = trace.0.borrow();
    let lut_tails: Vec<&[u8]> = writes
        .windows(2)
        .filter_map(|window| {
            if window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
                && window[1].len() >= 105
            {
                Some(&window[1][100..105])
            } else {
                None
            }
        })
        .collect();
    assert_eq!(lut_tails, [&[0x7f; 5][..], &[0xbf; 5][..]]);
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION])
            .count(),
        2
    );
}

#[test]
fn prepared_rows_write_only_the_requested_window_and_exact_bytes() {
    let (_, mut panel, trace) = fixture();
    let mut rows = [0_u8; 300];
    for (index, byte) in rows.iter_mut().enumerate() {
        *byte = index as u8;
    }

    panel
        .write_prepared_rows(17, &rows, PanelRamTarget::Black)
        .unwrap();

    let writes = trace.0.borrow();
    assert!(writes.windows(2).any(|pair| {
        pair[0].as_slice() == [ssd1677_driver::Command::SET_RAM_Y_ADDR]
            && pair[1].as_slice() == [17, 0, 19, 0]
    }));
    let command = writes
        .iter()
        .position(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
        .unwrap();
    assert_eq!(writes[command + 1..command + 4].concat(), rows);
    assert_eq!(writes.len(), command + 4);
}

mod common;
use common::block_on;

#[derive(Clone, Default)]
struct Trace(Rc<RefCell<Vec<Vec<u8>>>>);

struct Spi(Trace);
impl SpiErrorType for Spi {
    type Error = Infallible;
}
impl SpiDevice<u8> for Spi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for operation in operations {
            if let Operation::Write(bytes) = operation {
                self.0 .0.borrow_mut().push(bytes.to_vec());
            }
        }
        Ok(())
    }
}

#[test]
fn each_fixture_plane_decodes_exactly_with_small_input_buffer() {
    let (mut book, _, _) = fixture();
    let number = book.page_number(0).unwrap();
    let mut record = [0_u8; binbook_core::PAGE_RECORD_SIZE];
    let page = book.page(number, &mut record).unwrap();
    for slot in [
        binbook_core::PlaneSlot::OverlayMsb,
        binbook_core::PlaneSlot::OverlayLsb,
        binbook_core::PlaneSlot::FastBase,
    ] {
        let plane = page.planes.get(slot).unwrap();
        let mut encoded = vec![0_u8; plane.length.get() as usize];
        book.read_plane(plane, &mut encoded).unwrap();
        let mut exact = vec![0_u8; 48_000];
        binbook_decompress::decode_exact(plane.compression, &encoded, &mut exact)
            .unwrap_or_else(|error| panic!("whole {slot:?}: {error:?}"));
        for input_size in [1024, 32, 1] {
            let mut decoder = PlaneDecoder::new(plane);
            let mut input = vec![0_u8; input_size];
            let mut row = [0_u8; 100];
            for row_index in 0..480 {
                decoder
                    .fill(&mut book, &mut input, &mut row)
                    .unwrap_or_else(|error| {
                        panic!("{slot:?} input {input_size} row {row_index}: {error:?}")
                    });
            }
            decoder
                .finish()
                .unwrap_or_else(|error| panic!("{slot:?} input {input_size}: {error:?}"));
        }
    }
}

#[test]
fn owner_streaming_decoder_pauses_for_bounded_reads_and_matches_whole_decode() {
    let (mut book, _, _) = fixture();
    let number = book.page_number(0).unwrap();
    let mut record = [0_u8; binbook_core::PAGE_RECORD_SIZE];
    let page = book.page(number, &mut record).unwrap();
    let plane = page.planes.get(binbook_core::PlaneSlot::FastBase).unwrap();
    let mut encoded = vec![0_u8; plane.length.get() as usize];
    book.read_plane(plane, &mut encoded).unwrap();
    let mut expected = vec![0_u8; 48_000];
    binbook_decompress::decode_exact(plane.compression, &encoded, &mut expected).unwrap();

    let mut decoder = StreamingPlaneDecoder::new(plane);
    let mut input = [0u8; 37];
    let mut row = [0u8; 100];
    let mut actual = Vec::new();
    while actual.len() < expected.len() {
        match decoder.decode_row(&input, &mut row).unwrap() {
            PlaneDecodeStep::NeedInput => {
                let request = decoder.next_read(input.len()).unwrap();
                let start = request.offset as usize;
                input[..request.len].copy_from_slice(&encoded[start..start + request.len]);
                decoder.accept_read(request.len).unwrap();
            }
            PlaneDecodeStep::RowComplete => actual.extend_from_slice(&row),
        }
    }
    decoder.finish().unwrap();
    assert_eq!(actual, expected);
}

#[derive(Default)]
struct Pin;
impl DigitalErrorType for Pin {
    type Error = Infallible;
}
impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
impl InputPin for Pin {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[derive(Default)]
struct Delay(Vec<u32>);
impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}

fn fixture() -> (
    binbook_core::Book<SliceSource<'static>>,
    X4Panel<Spi, Pin, Pin, Pin>,
    Trace,
) {
    let bytes = include_bytes!("fixtures/nav_probe.binbook");
    let mut scratch = [0_u8; 1024];
    let book = binbook_core::Book::open(SliceSource::new(bytes), &mut scratch).unwrap();
    let trace = Trace::default();
    (book, X4Panel::new(Spi(trace.clone()), Pin, Pin, Pin), trace)
}

fn gray1_fixture() -> (
    binbook_core::Book<SliceSource<'static>>,
    X4Panel<Spi, Pin, Pin, Pin>,
    Trace,
) {
    let bytes = include_bytes!("fixtures/gray1_probe.binbook");
    let mut scratch = [0_u8; 1024];
    let book = binbook_core::Book::open(SliceSource::new(bytes), &mut scratch).unwrap();
    let trace = Trace::default();
    (book, X4Panel::new(Spi(trace.clone()), Pin, Pin, Pin), trace)
}

fn buffers() -> ([u8; 96], [u8; 300], [u8; 100], [u8; 100]) {
    ([0; 96], [0; 300], [0; 100], [0; 100])
}

#[test]
fn packbits_stream_accepts_buffers_smaller_than_input_and_output() {
    let encoded = [0x82, 0xaa, 0x02, 1, 2, 3, 0x81, 0xbb];
    let mut source = SliceSource::new(&encoded);
    let mut compressed = [0_u8; 2];
    let mut decoded = [0_u8; 3];
    let mut black = [0_u8; 3];
    let mut red = [0_u8; 3];
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut output = [0_u8; 8];
    let mut offset = 0;
    decode_stream(
        &mut source,
        0,
        encoded.len(),
        CompressionMethod::RlePackBits,
        output.len(),
        &mut buffers,
        |chunk| {
            output[offset..offset + chunk.len()].copy_from_slice(chunk);
            offset += chunk.len();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, [0xaa, 0xaa, 0xaa, 1, 2, 3, 0xbb, 0xbb]);
}

#[test]
fn absolute_gray_streams_each_controller_plane_with_one_full_window() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    block_on(render_absolute_gray(
        &mut panel,
        &mut book,
        0,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert_eq!(delay.0.iter().filter(|ns| **ns == 0).count(), 1);
}

#[test]
fn compiler_gray1_streams_fast_base_to_both_rams_and_refreshes() {
    let (mut book, mut panel, trace) = gray1_fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    block_on(render_absolute_gray(
        &mut panel,
        &mut book,
        0,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();

    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
}

#[test]
fn bw_differential_streams_previous_and_target_planes_with_one_full_window_each() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    block_on(render_bw_differential(
        &mut panel,
        &mut book,
        0,
        1,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert_eq!(delay.0.iter().filter(|ns| **ns == 0).count(), 1);
}

#[test]
fn fast_preview_streams_bw_planes_and_activates_accepted_preview_lut() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    block_on(render_fast_preview(
        &mut panel,
        &mut book,
        0,
        1,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_LUT]));
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
            && window[1].len() >= 105
            && &window[1][100..105] == &[0x7f; 5]
    }));
    assert!(!writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
            && window[1].len() >= 105
            && &window[1][100..105] == &[0x6f; 5]
    }));
    assert!(!writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
            && window[1].len() >= 105
            && &window[1][100..105] == &[0x4f; 5]
    }));
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::DISPLAY_UPDATE_CTRL2]
            && window[1].as_slice()
                == [ssd1677_driver::Command::UPDATE_CTRL_STAGED_GRAYSCALE | 0xc0]
    }));
}

#[test]
fn ui_transition_reuses_one_framebuffer_and_activates_partial_not_full() {
    let (_, mut panel, trace) = fixture();
    let mut fb = Gray2Framebuffer::new();
    let mut delay = Delay::default();

    block_on(render_ui_transition(
        &mut panel,
        &mut fb,
        |target| target.set_pixel(12, 34, Gray2Color::BLACK),
        &mut delay,
    ))
    .unwrap();

    assert_eq!(fb.get_pixel(12, 34), Gray2Color::BLACK);
    assert_differential_activation(&trace);
}

#[test]
fn fast_ui_transition_reuses_one_framebuffer_and_runs_preview_then_cleanup() {
    let (_, mut panel, trace) = fixture();
    let mut fb = Gray2Framebuffer::new();
    let mut delay = Delay::default();

    block_on(render_ui_transition_fast(
        &mut panel,
        &mut fb,
        |target| target.set_pixel(12, 34, Gray2Color::BLACK),
        &mut delay,
    ))
    .unwrap();

    assert_eq!(fb.get_pixel(12, 34), Gray2Color::BLACK);
    let writes = trace.0.borrow();
    let lut_tails: Vec<&[u8]> = writes
        .windows(2)
        .filter_map(|window| {
            if window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
                && window[1].len() >= 105
            {
                Some(&window[1][100..105])
            } else {
                None
            }
        })
        .collect();
    assert_eq!(lut_tails, [&[0x7f; 5][..], &[0xbf; 5][..]]);
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION])
            .count(),
        2
    );
}

#[test]
fn menu_and_book_cross_transitions_use_differential_planes() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let fb = Gray2Framebuffer::new();
    let mut delay = Delay::default();

    block_on(render_ui_to_book_differential(
        &mut panel,
        &fb,
        &mut book,
        1,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();
    assert_differential_activation(&trace);

    trace.0.borrow_mut().clear();
    block_on(render_book_to_ui_differential(
        &mut panel,
        &mut book,
        1,
        &fb,
        &mut buffers,
        &mut delay,
    ))
    .unwrap();
    assert_differential_activation(&trace);
}

#[test]
fn differential_staging_does_not_activate_until_storage_can_be_released() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let fb = Gray2Framebuffer::new();
    let mut delay = Delay::default();

    stage_ui_base(&mut panel, &fb, DifferentialRamTarget::Previous).unwrap();
    stage_book_fast_base(
        &mut panel,
        &mut book,
        1,
        &mut buffers,
        DifferentialRamTarget::Target,
    )
    .unwrap();

    let writes = trace.0.borrow();
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
    drop(writes);

    block_on(activate_differential(&mut panel, &mut delay)).unwrap();
    assert_differential_activation(&trace);
}

#[test]
fn decoded_fast_base_can_be_staged_after_the_book_source_is_released() {
    let (mut book, mut panel, trace) = fixture();
    let mut compressed = [0_u8; 4096];
    let mut decoded = [0_u8; FAST_BASE_BYTES];
    let mut delay = Delay::default();

    decode_book_fast_base(&mut book, 1, &mut compressed, &mut decoded).unwrap();
    drop(book);
    stage_decoded_fast_base(&mut panel, &decoded, DifferentialRamTarget::Target).unwrap();

    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
    drop(writes);
    block_on(activate_differential(&mut panel, &mut delay)).unwrap();
}

fn assert_differential_activation(trace: &Trace) {
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        1
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        1
    );
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::DISPLAY_UPDATE_CTRL2]
            && window[1].as_slice() == [ssd1677_driver::Command::UPDATE_CTRL_FAST]
    }));
    assert!(!writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::DISPLAY_UPDATE_CTRL2]
            && window[1].as_slice() == [ssd1677_driver::Command::UPDATE_CTRL_NORMAL]
    }));
}

#[test]
fn fast_preview_cleanup_loads_accepted_cleanup_lut_and_activates_staged_refresh() {
    let (_, mut panel, trace) = fixture();
    let mut delay = Delay::default();
    block_on(cleanup_fast_preview(&mut panel, &mut delay)).unwrap();
    let writes = trace.0.borrow();
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED]));
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW]));
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::WRITE_LUT]
            && window[1].len() >= 105
            && &window[1][100..105] == &[0xbf; 5]
    }));
    assert!(writes.windows(2).any(|window| {
        window[0].as_slice() == [ssd1677_driver::Command::DISPLAY_UPDATE_CTRL2]
            && window[1].as_slice()
                == [ssd1677_driver::Command::UPDATE_CTRL_STAGED_GRAYSCALE | 0xc0]
    }));
}

#[test]
fn staged_overlay_cancels_only_between_strips_and_never_activates() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    let checks = std::cell::Cell::new(0);
    let outcome = block_on(render_staged_overlay(
        &mut panel,
        &mut book,
        0,
        &mut buffers,
        OverlayControl {
            expected_epoch: 7,
            epoch: || {
                let value = checks.get();
                checks.set(value + 1);
                if value >= 3 {
                    8
                } else {
                    7
                }
            },
            on_activate: || panic!("cancelled overlay must not activate"),
        },
        &mut delay,
    ))
    .unwrap();
    assert_eq!(outcome, OperationOutcome::Cancelled);
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        3
    );
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
}

#[test]
fn background_base_sync_is_cancellable_without_refresh_activation() {
    let (mut book, mut panel, trace) = fixture();
    let (mut compressed, mut decoded, mut black, mut red) = buffers();
    let mut buffers = RenderBuffers::new(&mut compressed, &mut decoded, &mut black, &mut red);
    let mut delay = Delay::default();
    let checks = std::cell::Cell::new(0);
    let outcome = block_on(sync_bw_base(
        &mut panel,
        &mut book,
        0,
        &mut buffers,
        11,
        || {
            let value = checks.get();
            checks.set(value + 1);
            if value >= 5 {
                12
            } else {
                11
            }
        },
        &mut delay,
    ))
    .unwrap();
    assert_eq!(outcome, OperationOutcome::Cancelled);
    let writes = trace.0.borrow();
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_RED])
            .count(),
        5
    );
    assert!(!writes
        .iter()
        .any(|write| write.as_slice() == [ssd1677_driver::Command::MASTER_ACTIVATION]));
}

#[test]
fn empty_caller_buffers_are_rejected_with_sizes() {
    let mut source = SliceSource::new(&[0_u8]);
    let mut empty = [];
    let mut decoded = [0_u8; 1];
    let mut black = [0_u8; 1];
    let mut red = [0_u8; 1];
    let mut buffers = RenderBuffers::new(&mut empty, &mut decoded, &mut black, &mut red);
    assert!(decode_stream(
        &mut source,
        0,
        1,
        CompressionMethod::None,
        1,
        &mut buffers,
        |_| Ok(())
    )
    .is_err());
}
