use binbook_diagnostic_protocol::{
    encode_frame, FrameHeader, FrameKind, Opcode, Status, FRAME_DELIMITER, MAX_FRAME_BYTES,
};

#[cfg(not(feature = "usb-rx-isr-buffer"))]
const SERIAL_RX_BUF_SIZE: usize = MAX_FRAME_BYTES * 2;
#[cfg(feature = "usb-rx-isr-buffer")]
const SERIAL_RX_BUF_SIZE: usize = MAX_FRAME_BYTES;
const SERIAL_TX_BUF_SIZE: usize = MAX_FRAME_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    BufferFull,
}

pub struct FeedResult {
    pub consumed: usize,
    pub frame_ready: bool,
}

pub struct SerialState {
    rx_buf: [u8; SERIAL_RX_BUF_SIZE],
    tx_buf: [u8; SERIAL_TX_BUF_SIZE],
    rx_pos: usize,
    tx_pos: usize,
    protocol_errors: u32,
    discard_until_delimiter: bool,
}

impl SerialState {
    pub const fn new() -> Self {
        Self {
            rx_buf: [0u8; SERIAL_RX_BUF_SIZE],
            tx_buf: [0u8; SERIAL_TX_BUF_SIZE],
            rx_pos: 0,
            tx_pos: 0,
            protocol_errors: 0,
            discard_until_delimiter: false,
        }
    }

    pub fn feed_rx(&mut self, bytes: &[u8]) -> FeedResult {
        let mut consumed = 0;
        let mut frame_ready = false;

        for &byte in bytes {
            if self.discard_until_delimiter {
                consumed += 1;
                if byte == FRAME_DELIMITER {
                    self.discard_until_delimiter = false;
                }
                continue;
            }

            if self.rx_pos >= SERIAL_RX_BUF_SIZE {
                self.note_protocol_error();
                self.discard_until_delimiter = true;
                self.rx_pos = 0;
                consumed += 1;
                continue;
            }

            self.rx_buf[self.rx_pos] = byte;
            self.rx_pos += 1;
            consumed += 1;

            if byte == FRAME_DELIMITER && !frame_ready {
                frame_ready = true;
            }
        }

        FeedResult {
            consumed,
            frame_ready,
        }
    }

    pub fn next_frame(&mut self, out: &mut [u8; MAX_FRAME_BYTES]) -> Option<usize> {
        if self.rx_pos == 0 {
            return None;
        }

        let delimiter_pos = self.rx_buf[..self.rx_pos]
            .iter()
            .position(|&b| b == FRAME_DELIMITER)?;

        let frame_len = delimiter_pos + 1;

        if frame_len > MAX_FRAME_BYTES {
            self.note_protocol_error();
            self.rx_buf.copy_within(frame_len..self.rx_pos, 0);
            self.rx_pos -= frame_len;
            return self.next_frame(out);
        }

        out[..frame_len].copy_from_slice(&self.rx_buf[..frame_len]);
        self.rx_buf.copy_within(frame_len..self.rx_pos, 0);
        self.rx_pos -= frame_len;

        Some(frame_len)
    }

    pub fn frame_ready(&self) -> bool {
        self.rx_buf[..self.rx_pos].contains(&FRAME_DELIMITER)
    }

    pub fn buffered_rx_bytes(&self) -> usize {
        self.rx_pos
    }

    pub fn note_rx_overflow(&mut self) {
        self.note_protocol_error();
        self.rx_pos = 0;
        self.discard_until_delimiter = true;
    }

    pub fn needs_service(&self, completion_pending: bool) -> bool {
        completion_pending || self.frame_ready() || self.tx_pos != 0
    }

    pub fn queue_tx(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.tx_pos + frame.len() > SERIAL_TX_BUF_SIZE {
            return Err(TransportError::BufferFull);
        }
        self.tx_buf[self.tx_pos..self.tx_pos + frame.len()].copy_from_slice(frame);
        self.tx_pos += frame.len();
        Ok(())
    }

    pub fn replace_tx(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > SERIAL_TX_BUF_SIZE {
            return Err(TransportError::BufferFull);
        }
        self.tx_buf[..frame.len()].copy_from_slice(frame);
        self.tx_pos = frame.len();
        Ok(())
    }

    pub fn replace_tx_response(
        &mut self,
        request: FrameHeader,
        status: Status,
        payload: &[u8],
    ) -> Result<(), TransportError> {
        let response = FrameHeader {
            kind: FrameKind::Response,
            opcode: request.opcode,
            status,
            sequence: request.sequence,
            payload_len: payload.len() as u16,
        };
        self.tx_pos = encode_frame(&response, payload, &mut self.tx_buf)
            .map_err(|_| TransportError::BufferFull)?;
        Ok(())
    }

    pub fn replace_tx_event(
        &mut self,
        opcode: Opcode,
        payload: &[u8],
    ) -> Result<(), TransportError> {
        let event = FrameHeader {
            kind: FrameKind::Event,
            opcode,
            status: Status::Ok,
            sequence: 0,
            payload_len: payload.len() as u16,
        };
        self.tx_pos = encode_frame(&event, payload, &mut self.tx_buf)
            .map_err(|_| TransportError::BufferFull)?;
        Ok(())
    }

    pub fn pending_tx(&self) -> &[u8] {
        &self.tx_buf[..self.tx_pos]
    }

    pub fn consume_tx(&mut self, written: usize) {
        if written >= self.tx_pos {
            self.tx_pos = 0;
        } else {
            let remaining = self.tx_pos - written;
            self.tx_buf.copy_within(written..self.tx_pos, 0);
            self.tx_pos = remaining;
        }
    }

    pub fn protocol_error_count(&self) -> u32 {
        self.protocol_errors
    }

    pub fn note_protocol_error(&mut self) {
        self.protocol_errors = self.protocol_errors.saturating_add(1);
    }
}

impl Default for SerialState {
    fn default() -> Self {
        Self::new()
    }
}
