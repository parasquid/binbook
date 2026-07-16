use embedded_sd_storage::{BlockIoEvent, BlockIoOperation, BlockIoPhase};
use portable_atomic::{AtomicU32, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockIoAggregate {
    pub read_calls: u32,
    pub read_blocks: u32,
    pub read_us: u32,
    pub write_calls: u32,
    pub write_blocks: u32,
    pub write_us: u32,
    pub anomalies: u32,
}

struct OperationAccumulator {
    active: AtomicU8,
    started_us: AtomicU32,
    calls: AtomicU32,
    blocks: AtomicU32,
    elapsed_us: AtomicU32,
}

impl OperationAccumulator {
    const fn new() -> Self {
        Self {
            active: AtomicU8::new(0),
            started_us: AtomicU32::new(0),
            calls: AtomicU32::new(0),
            blocks: AtomicU32::new(0),
            elapsed_us: AtomicU32::new(0),
        }
    }

    fn start(&self, now_us: u32) -> bool {
        if self
            .active
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        self.started_us.store(now_us, Ordering::Release);
        true
    }

    fn complete(&self, blocks: u16, now_us: u32) -> bool {
        if self.active.swap(0, Ordering::AcqRel) == 0 {
            return false;
        }
        let elapsed = now_us.wrapping_sub(self.started_us.load(Ordering::Acquire));
        saturating_add(&self.calls, 1);
        saturating_add(&self.blocks, u32::from(blocks));
        saturating_add(&self.elapsed_us, elapsed);
        true
    }

    fn take(&self) -> (u32, u32, u32) {
        (
            self.calls.swap(0, Ordering::AcqRel),
            self.blocks.swap(0, Ordering::AcqRel),
            self.elapsed_us.swap(0, Ordering::AcqRel),
        )
    }
}

pub struct BlockIoAccumulator {
    read: OperationAccumulator,
    write: OperationAccumulator,
    anomalies: AtomicU32,
}

impl BlockIoAccumulator {
    pub const fn new() -> Self {
        Self {
            read: OperationAccumulator::new(),
            write: OperationAccumulator::new(),
            anomalies: AtomicU32::new(0),
        }
    }

    pub fn observe(&self, event: BlockIoEvent, now_us: u64) {
        let operation = match event.operation {
            BlockIoOperation::Read => &self.read,
            BlockIoOperation::Write => &self.write,
        };
        let valid = match event.phase {
            BlockIoPhase::Start => operation.start(now_us as u32),
            BlockIoPhase::Complete => operation.complete(event.blocks, now_us as u32),
        };
        if !valid {
            saturating_add(&self.anomalies, 1);
        }
    }

    pub fn take(&self) -> BlockIoAggregate {
        let (read_calls, read_blocks, read_us) = self.read.take();
        let (write_calls, write_blocks, write_us) = self.write.take();
        BlockIoAggregate {
            read_calls,
            read_blocks,
            read_us,
            write_calls,
            write_blocks,
            write_us,
            anomalies: self.anomalies.swap(0, Ordering::AcqRel),
        }
    }
}

impl Default for BlockIoAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

fn saturating_add(target: &AtomicU32, value: u32) {
    let _ = target.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}
