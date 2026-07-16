#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use binbook_fw::sd_spi_trace::{
    SdWriteCapture, SpiOperationEvent, SpiOperationKind, SpiTraceObserver, TracedSpiDevice, CMD24,
    CMD25,
};
use embedded_hal::spi::{ErrorKind, ErrorType, Operation, SpiDevice};
use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};

fn block(phase: BlockIoPhase, start: u32, blocks: u16, success: bool) -> BlockIoEvent {
    BlockIoEvent {
        operation: BlockIoOperation::Write,
        phase,
        start_block: start,
        blocks,
        success,
    }
}

fn spi(
    kind: SpiOperationKind,
    len: u16,
    before: u8,
    after: u8,
    started_us: u32,
    completed_us: u32,
) -> SpiOperationEvent {
    SpiOperationEvent {
        kind,
        len,
        first_before: before,
        first_after: after,
        started_us,
        completed_us,
    }
}

#[test]
fn captures_exact_cmd24_payload_busy_setup_total_and_gap() {
    let mut capture = SdWriteCapture::<2>::new();
    capture.clear();
    capture.observe_block_io(block(BlockIoPhase::Start, 91, 1, false), 100);
    capture.observe_spi(spi(SpiOperationKind::Write, 6, 0x40 | CMD24, 0, 110, 114));
    capture.observe_spi(spi(SpiOperationKind::Write, 512, 0, 0, 120, 220));
    capture.observe_spi(spi(SpiOperationKind::TransferInPlace, 1, 0xff, 0, 225, 227));
    capture.observe_spi(spi(SpiOperationKind::TransferInPlace, 1, 0xff, 0, 240, 242));
    capture.observe_spi(spi(
        SpiOperationKind::TransferInPlace,
        1,
        0xff,
        0xff,
        255,
        257,
    ));
    capture.observe_spi(spi(SpiOperationKind::Transfer, 1, 0xff, 0, 260, 262));
    capture.observe_block_io(block(BlockIoPhase::Complete, 91, 1, true), 270);

    capture.observe_block_io(block(BlockIoPhase::Start, 92, 1, false), 300);
    capture.observe_spi(spi(SpiOperationKind::Write, 6, 0x40 | CMD24, 0, 310, 314));
    capture.observe_spi(spi(SpiOperationKind::Write, 512, 0, 0, 320, 420));
    capture.observe_block_io(block(BlockIoPhase::Complete, 92, 1, true), 450);

    assert_eq!(capture.record(0).unwrap().start_block, 91);
    assert_eq!(capture.record(0).unwrap().blocks, 1);
    assert_eq!(capture.record(0).unwrap().command, CMD24);
    assert_eq!(capture.record(0).unwrap().data_spi_us, 100);
    assert_eq!(capture.record(0).unwrap().busy_wait_us, 32);
    assert_eq!(capture.record(0).unwrap().command_setup_us, 38);
    assert_eq!(capture.record(0).unwrap().total_us, 170);
    assert_eq!(capture.record(1).unwrap().gap_us, 30);
    assert_eq!(capture.anomalies(), 0);
}

#[test]
fn completion_can_refine_semantic_purpose_after_committed_data_is_inspected() {
    let mut capture = SdWriteCapture::<1>::new();
    capture.clear();
    capture.observe_block_io_with_purpose(block(BlockIoPhase::Start, 91, 1, false), 100, 4);
    capture.observe_spi(spi(SpiOperationKind::Write, 6, 0x40 | CMD24, 0, 110, 114));
    capture.observe_spi(spi(SpiOperationKind::Write, 512, 0, 0, 120, 220));
    capture.observe_block_io_with_purpose(block(BlockIoPhase::Complete, 91, 1, true), 270, 5);

    assert_eq!(capture.record(0).unwrap().purpose, 5);
}

#[test]
fn captures_cmd25_multiple_payloads_and_reports_capacity_overflow() {
    let mut capture = SdWriteCapture::<1>::new();
    capture.clear();
    capture.observe_block_io(block(BlockIoPhase::Start, 400, 8, false), 10);
    capture.observe_spi(spi(SpiOperationKind::Write, 6, 0x40 | CMD25, 0, 20, 24));
    for index in 0..8 {
        let start = 30 + index * 70;
        capture.observe_spi(spi(SpiOperationKind::Write, 512, 0, 0, start, start + 60));
    }
    capture.observe_block_io(block(BlockIoPhase::Complete, 400, 8, true), 610);
    assert_eq!(capture.record(0).unwrap().command, CMD25);
    assert_eq!(capture.record(0).unwrap().data_spi_us, 480);

    capture.observe_block_io(block(BlockIoPhase::Start, 500, 1, false), 700);
    capture.observe_block_io(block(BlockIoPhase::Complete, 500, 1, false), 710);
    assert!(capture.overflowed());
}

#[test]
fn mismatched_and_truncated_sequences_are_explicit_anomalies() {
    let mut capture = SdWriteCapture::<2>::new();
    capture.clear();
    capture.observe_block_io(block(BlockIoPhase::Complete, 1, 1, false), 5);
    capture.observe_block_io(block(BlockIoPhase::Start, 10, 2, false), 10);
    capture.observe_block_io(block(BlockIoPhase::Start, 11, 1, false), 11);
    capture.observe_spi(spi(SpiOperationKind::Write, 512, 0, 0, 12, 20));
    capture.observe_spi(spi(SpiOperationKind::TransferInPlace, 1, 0xff, 0, 21, 22));
    capture.observe_block_io(block(BlockIoPhase::Complete, 12, 2, false), 30);
    assert!(capture.anomalies() >= 4);
    assert!(capture.record(0).unwrap().anomalies >= 3);
}

#[derive(Default)]
struct FakeSpi {
    transaction_calls: std::rc::Rc<core::cell::Cell<usize>>,
    last_operation_count: std::rc::Rc<core::cell::Cell<usize>>,
}

impl ErrorType for FakeSpi {
    type Error = ErrorKind;
}

impl SpiDevice<u8> for FakeSpi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.transaction_calls
            .set(self.transaction_calls.get().saturating_add(1));
        self.last_operation_count.set(operations.len());
        for operation in operations {
            if let Operation::TransferInPlace(bytes) = operation {
                bytes.fill(0x5a);
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Observer {
    now: core::cell::Cell<u32>,
    events: core::cell::RefCell<std::vec::Vec<SpiOperationEvent>>,
}

impl SpiTraceObserver for &Observer {
    fn now_us(&self) -> u32 {
        let now = self.now.get();
        self.now.set(now + 7);
        now
    }

    fn observe(&self, event: SpiOperationEvent) {
        self.events.borrow_mut().push(event);
    }
}

#[test]
fn traced_spi_records_pre_and_post_transfer_bytes_without_changing_semantics() {
    let observer = Observer::default();
    let mut device = TracedSpiDevice::new(FakeSpi::default(), &observer);
    let mut byte = [0xff];
    device.transfer_in_place(&mut byte).unwrap();
    assert_eq!(byte, [0x5a]);
    assert_eq!(
        observer.events.borrow().as_slice(),
        &[SpiOperationEvent {
            kind: SpiOperationKind::TransferInPlace,
            len: 1,
            first_before: 0xff,
            first_after: 0x5a,
            started_us: 0,
            completed_us: 7,
        }]
    );
}

#[test]
fn traced_spi_preserves_one_inner_transaction_for_multiple_operations() {
    let transaction_calls = std::rc::Rc::new(core::cell::Cell::new(0));
    let last_operation_count = std::rc::Rc::new(core::cell::Cell::new(0));
    let observer = Observer::default();
    let mut device = TracedSpiDevice::new(
        FakeSpi {
            transaction_calls: transaction_calls.clone(),
            last_operation_count: last_operation_count.clone(),
        },
        &observer,
    );
    let command = [0x40, 0, 0, 0, 0, 0x95];
    let mut response = [0xff; 2];

    device
        .transaction(&mut [
            Operation::Write(&command),
            Operation::TransferInPlace(&mut response),
        ])
        .unwrap();

    assert_eq!(transaction_calls.get(), 1);
    assert_eq!(last_operation_count.get(), 2);
    assert_eq!(response, [0x5a; 2]);
    assert_eq!(observer.events.borrow().len(), 2);
    assert_eq!(observer.events.borrow()[0].first_before, 0x40);
    assert_eq!(observer.events.borrow()[1].first_before, 0xff);
    assert_eq!(observer.events.borrow()[1].first_after, 0x5a);
}

#[test]
fn compact_capture_has_bounded_ram_and_never_silently_truncates_time() {
    assert!(core::mem::size_of::<SdWriteCapture<384>>() <= 9_400);
    let mut capture = SdWriteCapture::<1>::new();
    assert_eq!(capture.record(0), None);
    capture.clear();
    capture.observe_block_io(block(BlockIoPhase::Start, 1, 1, false), 0);
    capture.observe_spi(spi(SpiOperationKind::Write, 6, 0x40 | CMD24, 0, 1, 2));
    capture.observe_block_io(block(BlockIoPhase::Complete, 1, 1, true), 70_000);
    assert_eq!(capture.record(0).unwrap().total_us, u32::from(u16::MAX));
    assert_ne!(capture.record(0).unwrap().anomalies, 0);
    assert_ne!(capture.anomalies(), 0);
}
