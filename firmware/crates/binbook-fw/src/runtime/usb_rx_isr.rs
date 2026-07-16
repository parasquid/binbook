use core::{cell::RefCell, convert::Infallible};

use binbook_diagnostic_protocol::MAX_FRAME_BYTES;
use embassy_sync::{
    blocking_mutex::{raw::CriticalSectionRawMutex, Mutex},
    signal::Signal,
};
use esp_hal::{
    usb_serial_jtag::{UsbSerialJtag, UsbSerialJtagRx, UsbSerialJtagTx},
    Blocking,
};
use heapless::Deque;

const ISR_RX_BYTES: usize = MAX_FRAME_BYTES;

struct IsrRxState {
    rx: Option<UsbSerialJtagRx<'static, Blocking>>,
    bytes: Deque<u8, ISR_RX_BYTES>,
    overflowed: bool,
}

static RX_STATE: Mutex<CriticalSectionRawMutex, RefCell<IsrRxState>> =
    Mutex::new(RefCell::new(IsrRxState {
        rx: None,
        bytes: Deque::new(),
        overflowed: false,
    }));
static RX_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub(super) struct InterruptBufferedRx;

pub(super) struct BlockingTx {
    tx: UsbSerialJtagTx<'static, Blocking>,
}

pub(super) fn install(
    usb_device: esp_hal::peripherals::USB_DEVICE<'static>,
) -> (InterruptBufferedRx, BlockingTx) {
    let mut usb = UsbSerialJtag::new(usb_device);
    usb.set_interrupt_handler(usb_rx_handler);
    let (mut rx, tx) = usb.split();
    rx.listen_rx_packet_recv_interrupt();
    RX_STATE.lock(|state| state.borrow_mut().rx = Some(rx));
    (InterruptBufferedRx, BlockingTx { tx })
}

#[esp_hal::handler]
fn usb_rx_handler() {
    RX_STATE.lock(|state| {
        let mut state = state.borrow_mut();
        loop {
            let byte = match state.rx.as_mut().and_then(|rx| rx.read_byte().ok()) {
                Some(byte) => byte,
                None => break,
            };
            if state.bytes.push_back(byte).is_err() {
                state.overflowed = true;
            }
        }
        if let Some(rx) = state.rx.as_mut() {
            rx.reset_rx_packet_recv_interrupt();
        }
    });
    RX_READY.signal(());
}

impl InterruptBufferedRx {
    pub fn take_overflowed(&mut self) -> bool {
        RX_STATE.lock(|state| {
            let mut state = state.borrow_mut();
            let overflowed = state.overflowed;
            state.overflowed = false;
            overflowed
        })
    }

    pub fn drain_rx_fifo(&mut self, out: &mut [u8]) -> usize {
        RX_STATE.lock(|state| {
            let mut state = state.borrow_mut();
            let mut count = 0;
            while count < out.len() {
                let Some(byte) = state.bytes.pop_front() else {
                    break;
                };
                out[count] = byte;
                count += 1;
            }
            count
        })
    }
}

impl embedded_io_async::ErrorType for InterruptBufferedRx {
    type Error = Infallible;
}

impl embedded_io_async::Read for InterruptBufferedRx {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, Self::Error> {
        loop {
            let count = self.drain_rx_fifo(out);
            if count != 0 || out.is_empty() {
                return Ok(count);
            }
            RX_READY.wait().await;
        }
    }
}

impl embedded_io_async::ErrorType for BlockingTx {
    type Error = Infallible;
}

impl embedded_io_async::Write for BlockingTx {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        self.tx.write(bytes)?;
        Ok(bytes.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
