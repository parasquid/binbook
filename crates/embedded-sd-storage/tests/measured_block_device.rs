use std::{cell::RefCell, rc::Rc};

use embedded_sd_storage::{
    Block, BlockCount, BlockDevice, BlockIdx, BlockIoEvent, BlockIoObserver, BlockIoOperation,
    BlockIoPhase, MeasuredBlockDevice,
};

#[derive(Default)]
struct DeviceState {
    fail_write: bool,
}

#[derive(Clone, Default)]
struct Device(Rc<RefCell<DeviceState>>);

impl BlockDevice for Device {
    type Error = &'static str;

    fn read(&self, blocks: &mut [Block], _: BlockIdx) -> Result<(), Self::Error> {
        blocks.iter_mut().for_each(|block| block.contents.fill(7));
        Ok(())
    }

    fn write(&self, _: &[Block], _: BlockIdx) -> Result<(), Self::Error> {
        if self.0.borrow().fail_write {
            Err("write")
        } else {
            Ok(())
        }
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        Ok(BlockCount(16))
    }
}

#[derive(Clone, Default)]
struct Observer(Rc<RefCell<Vec<BlockIoEvent>>>);

impl BlockIoObserver for Observer {
    fn observe(&self, event: BlockIoEvent) {
        self.0.borrow_mut().push(event);
    }
}

type ObservedBlockData = Rc<RefCell<Vec<(u32, Vec<u8>)>>>;

#[derive(Clone, Default)]
struct DataObserver(ObservedBlockData);

impl BlockIoObserver for DataObserver {
    fn observe(&self, _: BlockIoEvent) {}

    fn observe_read_data(&self, start_block: u32, blocks: &[Block]) {
        self.0.borrow_mut().push((
            start_block,
            blocks.iter().map(|block| block.contents[0]).collect(),
        ));
    }

    fn observe_write_data(&self, start_block: u32, blocks: &[Block]) {
        self.0.borrow_mut().push((
            start_block,
            blocks.iter().map(|block| block.contents[0]).collect(),
        ));
    }
}

#[test]
fn successful_read_exposes_exact_returned_blocks_to_the_observer() {
    let observer = DataObserver::default();
    let observed = observer.0.clone();
    let measured = MeasuredBlockDevice::new(Device::default(), observer);
    let mut blocks = [Block::new(), Block::new()];

    measured.read(&mut blocks, BlockIdx(9)).unwrap();

    assert_eq!(*observed.borrow(), [(9, vec![7, 7])]);
}

#[test]
fn successful_write_exposes_exact_committed_blocks_to_the_observer() {
    let observer = DataObserver::default();
    let observed = observer.0.clone();
    let measured = MeasuredBlockDevice::new(Device::default(), observer);
    let mut blocks = [Block::new(), Block::new()];
    blocks[0].contents[0] = 11;
    blocks[1].contents[0] = 12;

    measured.write(&blocks, BlockIdx(13)).unwrap();

    assert_eq!(*observed.borrow(), [(13, vec![11, 12])]);
}

#[test]
fn failed_write_does_not_expose_uncommitted_data_to_the_observer() {
    let device = Device::default();
    device.0.borrow_mut().fail_write = true;
    let observer = DataObserver::default();
    let observed = observer.0.clone();
    let measured = MeasuredBlockDevice::new(device, observer);

    assert!(measured.write(&[Block::new()], BlockIdx(13)).is_err());
    assert!(observed.borrow().is_empty());
}

#[test]
fn reports_exact_multiblock_read_boundaries_without_changing_data() {
    let observer = Observer::default();
    let events = observer.0.clone();
    let measured = MeasuredBlockDevice::new(Device::default(), observer);
    let mut blocks = [Block::new(), Block::new(), Block::new()];

    measured.read(&mut blocks, BlockIdx(4)).unwrap();
    assert!(blocks.iter().all(|block| block.contents == [7; 512]));
    assert_eq!(
        *events.borrow(),
        [
            BlockIoEvent {
                operation: BlockIoOperation::Read,
                phase: BlockIoPhase::Start,
                start_block: 4,
                blocks: 3,
                success: false,
            },
            BlockIoEvent {
                operation: BlockIoOperation::Read,
                phase: BlockIoPhase::Complete,
                start_block: 4,
                blocks: 3,
                success: true,
            },
        ]
    );
}

#[test]
fn completion_reports_failed_write_without_hiding_device_error() {
    let device = Device::default();
    device.0.borrow_mut().fail_write = true;
    let observer = Observer::default();
    let events = observer.0.clone();
    let measured = MeasuredBlockDevice::new(device, observer);

    let blocks: [Block; 2] = core::array::from_fn(|_| Block::new());
    assert_eq!(measured.write(&blocks, BlockIdx(0)), Err("write"));
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(events.borrow()[1].operation, BlockIoOperation::Write);
    assert_eq!(events.borrow()[1].phase, BlockIoPhase::Complete);
    assert!(!events.borrow()[1].success);
}
