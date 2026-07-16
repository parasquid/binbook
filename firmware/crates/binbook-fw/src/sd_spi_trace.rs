use embedded_hal::spi::{ErrorType, Operation, SpiDevice};
use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};

pub const CMD24: u8 = 24;
pub const CMD25: u8 = 25;
const MAX_TRACED_OPERATIONS_PER_TRANSACTION: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiOperationKind {
    Read,
    Write,
    Transfer,
    TransferInPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpiOperationEvent {
    pub kind: SpiOperationKind,
    pub len: u16,
    pub first_before: u8,
    pub first_after: u8,
    pub started_us: u32,
    pub completed_us: u32,
}

#[derive(Clone, Copy)]
struct PendingSpiOperationEvent {
    kind: SpiOperationKind,
    len: u16,
    first_before: u8,
    started_us: u32,
}

const EMPTY_PENDING_SPI_OPERATION: PendingSpiOperationEvent = PendingSpiOperationEvent {
    kind: SpiOperationKind::Write,
    len: 0,
    first_before: 0,
    started_us: 0,
};

pub trait SpiTraceObserver {
    fn now_us(&self) -> u32;
    fn observe(&self, event: SpiOperationEvent);
}

pub struct TracedSpiDevice<D, O> {
    inner: D,
    observer: O,
}

impl<D, O> TracedSpiDevice<D, O> {
    pub const fn new(inner: D, observer: O) -> Self {
        Self { inner, observer }
    }

    pub fn into_inner(self) -> D {
        self.inner
    }
}

impl<D, O> ErrorType for TracedSpiDevice<D, O>
where
    D: ErrorType,
{
    type Error = D::Error;
}

impl<D, O> SpiDevice<u8> for TracedSpiDevice<D, O>
where
    D: SpiDevice<u8>,
    O: SpiTraceObserver,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        let traced_len = operations.len().min(MAX_TRACED_OPERATIONS_PER_TRANSACTION);
        let mut pending = [EMPTY_PENDING_SPI_OPERATION; MAX_TRACED_OPERATIONS_PER_TRANSACTION];
        for (slot, operation) in pending.iter_mut().zip(operations.iter()) {
            let (kind, len, first_before) = operation_summary(operation);
            *slot = PendingSpiOperationEvent {
                kind,
                len: len.min(usize::from(u16::MAX)) as u16,
                first_before,
                started_us: self.observer.now_us(),
            };
        }

        let result = self.inner.transaction(operations);

        for (operation, event) in operations.iter().zip(pending.iter()).take(traced_len) {
            let completed_us = self.observer.now_us();
            let first_after = operation_first(operation);
            self.observer.observe(SpiOperationEvent {
                kind: event.kind,
                len: event.len,
                first_before: event.first_before,
                first_after,
                started_us: event.started_us,
                completed_us,
            });
        }
        result
    }
}

fn operation_summary(operation: &Operation<'_, u8>) -> (SpiOperationKind, usize, u8) {
    match operation {
        Operation::Read(bytes) => (SpiOperationKind::Read, bytes.len(), 0),
        Operation::Write(bytes) => (
            SpiOperationKind::Write,
            bytes.len(),
            bytes.first().copied().unwrap_or(0),
        ),
        Operation::Transfer(_, write) => (
            SpiOperationKind::Transfer,
            write.len(),
            write.first().copied().unwrap_or(0),
        ),
        Operation::TransferInPlace(bytes) => (
            SpiOperationKind::TransferInPlace,
            bytes.len(),
            bytes.first().copied().unwrap_or(0),
        ),
        Operation::DelayNs(_) => (SpiOperationKind::Write, 0, 0),
    }
}

fn operation_first(operation: &Operation<'_, u8>) -> u8 {
    match operation {
        Operation::Read(bytes) | Operation::TransferInPlace(bytes) => {
            bytes.first().copied().unwrap_or(0)
        }
        Operation::Transfer(read, _) => read.first().copied().unwrap_or(0),
        Operation::Write(bytes) => bytes.first().copied().unwrap_or(0),
        Operation::DelayNs(_) => 0,
    }
}

impl SpiOperationEvent {
    pub const fn elapsed_us(self) -> u32 {
        self.completed_us.wrapping_sub(self.started_us)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SdWriteTransaction {
    pub start_block: u32,
    pub blocks: u16,
    pub command: u8,
    pub command_setup_us: u32,
    pub data_spi_us: u32,
    pub busy_wait_us: u32,
    pub total_us: u32,
    pub gap_us: u32,
    pub success: bool,
    pub anomalies: u8,
    pub purpose: u8,
}

#[derive(Debug, Clone, Copy, Default)]
struct CompactSdWriteTransaction {
    start_block: u32,
    blocks: u16,
    command: u8,
    flags: u8,
    command_setup_us: u16,
    data_spi_us: u16,
    busy_wait_us: u16,
    total_us: u16,
    gap_us: u32,
    purpose: u8,
}

#[derive(Debug, Clone, Copy)]
struct ActiveWrite {
    record: SdWriteTransaction,
    started_us: u32,
    busy_started_us: Option<u32>,
    awaiting_busy: bool,
}

pub struct SdWriteCapture<const N: usize> {
    records: [CompactSdWriteTransaction; N],
    len: usize,
    overflowed: bool,
    anomalies: u32,
    active: Option<ActiveWrite>,
    previous_completed_us: Option<u32>,
    enabled: bool,
}

impl<const N: usize> SdWriteCapture<N> {
    pub const fn new() -> Self {
        Self {
            records: [CompactSdWriteTransaction {
                start_block: 0,
                blocks: 0,
                command: 0,
                flags: 0,
                command_setup_us: 0,
                data_spi_us: 0,
                busy_wait_us: 0,
                total_us: 0,
                gap_us: 0,
                purpose: 0,
            }; N],
            len: 0,
            overflowed: false,
            anomalies: 0,
            active: None,
            previous_completed_us: None,
            enabled: false,
        }
    }

    pub fn observe_block_io(&mut self, event: BlockIoEvent, now_us: u32) {
        self.observe_block_io_with_purpose(event, now_us, 0);
    }

    pub fn observe_block_io_with_purpose(&mut self, event: BlockIoEvent, now_us: u32, purpose: u8) {
        if event.operation != BlockIoOperation::Write {
            return;
        }
        if !self.enabled && self.active.is_none() {
            return;
        }
        match event.phase {
            BlockIoPhase::Start => {
                if self.active.is_some() {
                    self.flag_anomaly();
                    return;
                }
                self.active = Some(ActiveWrite {
                    record: SdWriteTransaction {
                        start_block: event.start_block,
                        blocks: event.blocks,
                        gap_us: self
                            .previous_completed_us
                            .map_or(0, |previous| now_us.wrapping_sub(previous)),
                        ..SdWriteTransaction::default()
                    },
                    started_us: now_us,
                    busy_started_us: None,
                    awaiting_busy: false,
                });
                if let Some(active) = self.active.as_mut() {
                    active.record.purpose = purpose;
                }
            }
            BlockIoPhase::Complete => {
                let Some(mut active) = self.active.take() else {
                    self.flag_anomaly();
                    return;
                };
                if purpose != 0 {
                    active.record.purpose = purpose;
                }
                if active.record.start_block != event.start_block
                    || active.record.blocks != event.blocks
                {
                    active.record.anomalies = active.record.anomalies.saturating_add(1);
                    self.anomalies = self.anomalies.saturating_add(1);
                }
                if let Some(started) = active.busy_started_us.take() {
                    active.record.busy_wait_us = active
                        .record
                        .busy_wait_us
                        .saturating_add(now_us.wrapping_sub(started));
                    active.record.anomalies = active.record.anomalies.saturating_add(1);
                    self.anomalies = self.anomalies.saturating_add(1);
                }
                active.record.total_us = now_us.wrapping_sub(active.started_us);
                active.record.success = event.success;
                active.record.command_setup_us = active
                    .record
                    .total_us
                    .saturating_sub(active.record.data_spi_us)
                    .saturating_sub(active.record.busy_wait_us);
                if active.record.command == 0 {
                    active.record.anomalies = active.record.anomalies.saturating_add(1);
                    self.anomalies = self.anomalies.saturating_add(1);
                }
                self.push(active.record);
                self.previous_completed_us = Some(now_us);
            }
        }
    }

    pub fn observe_spi(&mut self, event: SpiOperationEvent) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if event.kind == SpiOperationKind::Write
            && event.len == 6
            && event.first_before & 0xc0 == 0x40
        {
            let command = event.first_before & 0x3f;
            if command == CMD24 || command == CMD25 {
                active.record.command = command;
            }
        }
        if event.kind == SpiOperationKind::Write && event.len == 512 {
            active.record.data_spi_us =
                active.record.data_spi_us.saturating_add(event.elapsed_us());
            active.awaiting_busy = true;
            return;
        }
        if active.awaiting_busy
            && matches!(
                event.kind,
                SpiOperationKind::Transfer | SpiOperationKind::TransferInPlace
            )
            && event.len == 1
            && event.first_before == 0xff
        {
            match (active.busy_started_us, event.first_after) {
                (None, 0xff) => {}
                (None, _) => active.busy_started_us = Some(event.started_us),
                (Some(started), 0xff) => {
                    active.record.busy_wait_us = active
                        .record
                        .busy_wait_us
                        .saturating_add(event.completed_us.wrapping_sub(started));
                    active.busy_started_us = None;
                    active.awaiting_busy = false;
                }
                (Some(_), _) => {}
            }
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn record(&self, index: usize) -> Option<SdWriteTransaction> {
        if index >= self.len {
            return None;
        }
        let value = *self.records.get(index)?;
        Some(SdWriteTransaction {
            start_block: value.start_block,
            blocks: value.blocks,
            command: value.command,
            command_setup_us: u32::from(value.command_setup_us),
            data_spi_us: u32::from(value.data_spi_us),
            busy_wait_us: u32::from(value.busy_wait_us),
            total_us: u32::from(value.total_us),
            gap_us: value.gap_us,
            success: value.flags & 1 != 0,
            anomalies: value.flags >> 1,
            purpose: value.purpose,
        })
    }

    pub const fn capacity(&self) -> usize {
        N
    }

    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub const fn anomalies(&self) -> u32 {
        self.anomalies
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.overflowed = false;
        self.anomalies = 0;
        self.active = None;
        self.previous_completed_us = None;
        self.enabled = true;
    }

    pub fn freeze(&mut self) {
        self.enabled = false;
    }

    fn push(&mut self, mut record: SdWriteTransaction) {
        if self.len == N {
            self.overflowed = true;
            return;
        }
        let compact = CompactSdWriteTransaction {
            start_block: record.start_block,
            blocks: record.blocks,
            command: record.command,
            flags: u8::from(record.success) | (record.anomalies.min(0x7f) << 1),
            command_setup_us: compact_time(
                record.command_setup_us,
                &mut record,
                &mut self.anomalies,
            ),
            data_spi_us: compact_time(record.data_spi_us, &mut record, &mut self.anomalies),
            busy_wait_us: compact_time(record.busy_wait_us, &mut record, &mut self.anomalies),
            total_us: compact_time(record.total_us, &mut record, &mut self.anomalies),
            gap_us: record.gap_us,
            purpose: record.purpose,
        };
        let mut compact = compact;
        compact.flags = u8::from(record.success) | (record.anomalies.min(0x7f) << 1);
        self.records[self.len] = compact;
        self.len += 1;
    }

    fn flag_anomaly(&mut self) {
        self.anomalies = self.anomalies.saturating_add(1);
        if let Some(active) = self.active.as_mut() {
            active.record.anomalies = active.record.anomalies.saturating_add(1);
        }
    }
}

fn compact_time(value: u32, record: &mut SdWriteTransaction, anomalies: &mut u32) -> u16 {
    match u16::try_from(value) {
        Ok(value) => value,
        Err(_) => {
            record.anomalies = record.anomalies.saturating_add(1);
            *anomalies = anomalies.saturating_add(1);
            u16::MAX
        }
    }
}

impl<const N: usize> Default for SdWriteCapture<N> {
    fn default() -> Self {
        Self::new()
    }
}
