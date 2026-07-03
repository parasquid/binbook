mod common;

use core::convert::Infallible;
use embedded_hal::{
    digital::{ErrorType as DigitalErrorType, InputPin, OutputPin},
    spi::{ErrorType as SpiErrorType, Operation, SpiDevice},
};
use ssd1677_driver::{BusyWaitObserver, BusyWaitOutcome, Command};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xteink_x4_display::panel::X4Panel;
use xteink_x4_display::probes::{
    build_corner_row, corner_windows, custom_lut_candidate_waveform, custom_lut_probe_observed,
    partial_window_busy_cases, probe_polarity, CustomLutCandidate, PartialWindowPolarity,
    DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH,
};

#[test]
fn corner_probe_covers_each_physical_corner() {
    let windows = corner_windows();
    assert_eq!(windows[0].0, 0);
    assert_eq!(windows[0].1, 0);
    assert_eq!(windows[3].0 + windows[3].2, DISPLAY_WIDTH);
    assert_eq!(windows[3].1 + windows[3].3, DISPLAY_HEIGHT);
    let mut row = [0_u8; DISPLAY_ROW_BYTES];
    build_corner_row(0, &mut row);
    assert_eq!(&row[..16], &[0; 16]);
    assert_eq!(&row[84..], &[0; 16]);
}

#[test]
fn probe_kinds_include_full_refresh_and_clear() {
    use xteink_x4_display::probes::ProbeKind;
    assert_ne!(ProbeKind::FullRefreshCurrent, ProbeKind::ClearWhite);
    assert_ne!(ProbeKind::ClearWhite, ProbeKind::WindowCorners);
    assert_ne!(ProbeKind::WindowCorners, ProbeKind::PartialWindowBusyMatrix);
    assert_ne!(
        ProbeKind::PartialWindowBusyMatrix,
        ProbeKind::CustomLutFastA
    );
    assert_ne!(ProbeKind::CustomLutFastA, ProbeKind::CustomLutFastB);
}

#[test]
fn partial_window_busy_matrix_uses_stable_geometries_and_byte_counts() {
    let cases = partial_window_busy_cases();
    assert_eq!(cases.len(), 6);
    assert_eq!(cases[0].name, "full");
    assert_eq!(
        (cases[0].x, cases[0].y, cases[0].width, cases[0].height),
        (0, 0, 800, 480)
    );
    assert_eq!(cases[0].bytes_per_plane(), 48_000);
    assert_eq!(cases[1].name, "half-height");
    assert_eq!(
        (cases[1].x, cases[1].y, cases[1].width, cases[1].height),
        (0, 120, 800, 240)
    );
    assert_eq!(cases[1].bytes_per_plane(), 24_000);
    assert_eq!(cases[2].name, "stripe-64");
    assert_eq!(
        (cases[2].x, cases[2].y, cases[2].width, cases[2].height),
        (0, 208, 800, 64)
    );
    assert_eq!(cases[2].bytes_per_plane(), 6_400);
    assert_eq!(cases[3].name, "strip-16");
    assert_eq!(
        (cases[3].x, cases[3].y, cases[3].width, cases[3].height),
        (0, 232, 800, 16)
    );
    assert_eq!(cases[3].bytes_per_plane(), 1_600);
    assert_eq!(cases[4].name, "center-box");
    assert_eq!(
        (cases[4].x, cases[4].y, cases[4].width, cases[4].height),
        (300, 190, 200, 100)
    );
    assert_eq!(cases[4].bytes_per_plane(), 2_500);
    assert_eq!(cases[5].name, "page-number-box");
    assert_eq!(
        (cases[5].x, cases[5].y, cases[5].width, cases[5].height),
        (280, 0, 240, 80)
    );
    assert_eq!(cases[5].bytes_per_plane(), 2_400);
}

#[test]
fn partial_window_probe_alternates_real_pixel_transitions() {
    assert_eq!(probe_polarity(1), PartialWindowPolarity::BlackOnWhite);
    assert_eq!(probe_polarity(2), PartialWindowPolarity::WhiteOnBlack);
    assert_eq!(probe_polarity(3), PartialWindowPolarity::BlackOnWhite);
}

#[test]
fn custom_lut_candidates_preserve_voltage_tail_and_reduce_repeat_bytes() {
    let fast_a = custom_lut_candidate_waveform(CustomLutCandidate::FastA);
    let fast_b = custom_lut_candidate_waveform(CustomLutCandidate::FastB);
    assert_eq!(fast_a.lut.len(), 105);
    assert_eq!(fast_b.lut.len(), 105);
    assert_eq!(fast_a.gate_voltage, 0x17);
    assert_eq!(fast_b.gate_voltage, 0x17);
    assert_eq!(fast_a.source_voltage, [0x41, 0xa8, 0x32]);
    assert_eq!(fast_b.source_voltage, [0x41, 0xa8, 0x32]);
    assert_eq!(fast_a.vcom_voltage, 0x30);
    assert_eq!(fast_b.vcom_voltage, 0x30);
    assert_eq!(&fast_a.lut[100..105], &[0x6f; 5]);
    assert_eq!(&fast_b.lut[100..105], &[0x4f; 5]);
}

#[test]
fn custom_lut_probe_loads_candidate_and_triggers_staged_activation() {
    let trace = Trace::default();
    let busy = Busy::default();
    let mut panel = X4Panel::new(MockSpi(trace.clone()), Pin, Pin, busy);
    let mut delay = Delay::default();
    let mut observer = Observer::default();

    common::block_on(custom_lut_probe_observed(
        &mut panel,
        CustomLutCandidate::FastA,
        &mut delay,
        &mut observer,
    ))
    .unwrap();

    let writes = trace.0.borrow();
    assert_eq!(data_after(&writes, Command::WRITE_LUT).len(), 105);
    assert_eq!(
        &data_after(&writes, Command::WRITE_LUT)[100..105],
        &[0x6f; 5]
    );
    assert_eq!(data_after(&writes, Command::GATE_VOLTAGE), [0x17]);
    assert_eq!(
        data_after(&writes, Command::SOURCE_VOLTAGE),
        [0x41, 0xa8, 0x32]
    );
    assert_eq!(data_after(&writes, Command::VCOM_VOLTAGE), [0x30]);
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [Command::WRITE_RAM_RED]));
    assert!(writes
        .iter()
        .any(|write| write.as_slice() == [Command::WRITE_RAM_BW]));
    assert!(writes.windows(4).any(|window| {
        window[0].as_slice() == [Command::DISPLAY_UPDATE_CTRL1]
            && window[1].as_slice() == [0x00]
            && window[2].as_slice() == [Command::DISPLAY_UPDATE_CTRL2]
            && (window[3].as_slice() == [0x0c] || window[3].as_slice() == [0xcc])
    }));
    assert_eq!(observer.ends, [(0, BusyWaitOutcome::Ready)]);
}

#[derive(Clone, Default)]
struct Trace(Rc<RefCell<Vec<Vec<u8>>>>);

#[derive(Default)]
struct MockSpi(Trace);

impl SpiErrorType for MockSpi {
    type Error = Infallible;
}

impl SpiDevice<u8> for MockSpi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for operation in operations {
            match operation {
                Operation::Write(bytes) => self.0 .0.borrow_mut().push(bytes.to_vec()),
                Operation::Read(bytes) | Operation::Transfer(bytes, _) => bytes.fill(0),
                Operation::TransferInPlace(bytes) => bytes.fill(0),
                Operation::DelayNs(_) => {}
            }
        }
        Ok(())
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

#[derive(Clone, Default)]
struct Busy(Rc<Cell<bool>>);

impl DigitalErrorType for Busy {
    type Error = Infallible;
}

impl InputPin for Busy {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.get())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.0.get())
    }
}

#[derive(Default)]
struct Delay(Vec<u32>);

impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}

#[derive(Default)]
struct Observer {
    ends: Vec<(u32, BusyWaitOutcome)>,
}

impl BusyWaitObserver for Observer {
    fn busy_wait_start(&mut self, _timeout_ms: u32, _busy_state: Option<bool>) {}

    fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: BusyWaitOutcome) {
        self.ends.push((elapsed_ms, outcome));
    }
}

fn data_after(writes: &[Vec<u8>], command: u8) -> &[u8] {
    let index = writes
        .iter()
        .position(|write| write.as_slice() == [command])
        .expect("command must be written");
    writes[index + 1].as_slice()
}
