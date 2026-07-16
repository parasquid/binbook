use std::collections::VecDeque;

use binbook_diagnostic_protocol::{
    decode_frame, FrameHeader, FrameKind, Opcode, Status, FRAME_DELIMITER, MAX_FRAME_BYTES,
    MAX_PAYLOAD_BYTES,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedFrame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RouterError {
    #[error("malformed diagnostic frame")]
    MalformedFrame,
    #[error(
        "diagnostic frame exceeds the negotiated maximum: actual={actual} max={max} complete={complete}"
    )]
    FrameTooLarge {
        actual: usize,
        max: usize,
        complete: bool,
    },
    #[error("diagnostic frame queue is full")]
    QueueFull,
    #[error("duplicate response for sequence {0}")]
    DuplicateResponse(u16),
    #[error("expected {expected:?} for sequence {sequence}, received {actual:?}")]
    WrongOpcode {
        expected: Opcode,
        actual: Opcode,
        sequence: u16,
    },
    #[error("unexpected frame kind {0:?}")]
    UnexpectedKind(FrameKind),
    #[error("bulk event used nonzero sequence {0}")]
    EventSequence(u16),
    #[error("bulk event used non-OK frame status {0:?}")]
    EventStatus(Status),
}

#[derive(Debug, Default)]
pub struct FrameRouter {
    buffered: Vec<u8>,
    frames: VecDeque<RoutedFrame>,
}

impl FrameRouter {
    pub const MAX_QUEUED_FRAMES: usize = 16;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), RouterError> {
        self.buffered.extend_from_slice(bytes);
        while let Some(end) = self
            .buffered
            .iter()
            .position(|byte| *byte == FRAME_DELIMITER)
        {
            let encoded: Vec<u8> = self.buffered.drain(..=end).collect();
            if encoded.len() > MAX_FRAME_BYTES {
                return Err(RouterError::FrameTooLarge {
                    actual: encoded.len(),
                    max: MAX_FRAME_BYTES,
                    complete: true,
                });
            }
            if self.frames.len() == Self::MAX_QUEUED_FRAMES {
                return Err(RouterError::QueueFull);
            }
            let mut payload = [0; MAX_PAYLOAD_BYTES];
            let (header, payload_len) =
                decode_frame(&encoded, &mut payload).map_err(|_| RouterError::MalformedFrame)?;
            self.frames.push_back(RoutedFrame {
                header,
                payload: payload[..payload_len].to_vec(),
            });
        }
        if self.buffered.len() > MAX_FRAME_BYTES {
            let actual = self.buffered.len();
            self.buffered.clear();
            return Err(RouterError::FrameTooLarge {
                actual,
                max: MAX_FRAME_BYTES,
                complete: false,
            });
        }
        Ok(())
    }

    pub fn take_response(
        &mut self,
        opcode: Opcode,
        sequence: u16,
    ) -> Result<Option<RoutedFrame>, RouterError> {
        let matching = self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.header.sequence == sequence)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(RouterError::DuplicateResponse(sequence));
        }
        let Some(index) = matching.first().copied() else {
            return Ok(None);
        };
        let header = self.frames[index].header;
        if header.kind != FrameKind::Response {
            return Err(RouterError::UnexpectedKind(header.kind));
        }
        if header.opcode != opcode {
            return Err(RouterError::WrongOpcode {
                expected: opcode,
                actual: header.opcode,
                sequence,
            });
        }
        Ok(self.frames.remove(index))
    }

    pub fn take_bulk_event(&mut self, opcode: Opcode) -> Result<Option<RoutedFrame>, RouterError> {
        let Some(index) = self
            .frames
            .iter()
            .position(|frame| frame.header.opcode == opcode)
        else {
            return Ok(None);
        };
        let header = self.frames[index].header;
        if header.kind != FrameKind::Event {
            return Err(RouterError::UnexpectedKind(header.kind));
        }
        if header.sequence != 0 {
            return Err(RouterError::EventSequence(header.sequence));
        }
        if header.status != Status::Ok {
            return Err(RouterError::EventStatus(header.status));
        }
        Ok(self.frames.remove(index))
    }

    pub fn pending_frames(&self) -> usize {
        self.frames.len()
    }
}
