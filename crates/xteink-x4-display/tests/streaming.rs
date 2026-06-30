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
    page_source::PlaneDecoder,
    panel::X4Panel,
    render::{render_absolute_gray, render_staged_overlay, sync_bw_base, OverlayControl},
    stream::decode_stream,
};

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
fn absolute_gray_reconstructs_both_controller_planes_in_sixteen_row_strips() {
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
        30
    );
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.as_slice() == [ssd1677_driver::Command::WRITE_RAM_BW])
            .count(),
        30
    );
    assert_eq!(delay.0.iter().filter(|ns| **ns == 0).count(), 59);
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
