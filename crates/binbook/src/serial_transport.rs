use binbook_diagnostic_protocol::{
    decode_frame, FrameKind, Opcode, Status, FRAME_DELIMITER, MAX_FRAME_BYTES,
};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

pub struct SerialSession {
    port: Box<dyn serialport::SerialPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedResponse {
    pub sequence: u16,
    pub elapsed_ms: u128,
    pub frame: Vec<u8>,
}

impl SerialSession {
    pub fn open(port_path: &str) -> Result<Self, String> {
        let port = serialport::new(port_path, 115_200)
            .timeout(Duration::from_secs(2))
            .open()
            .map_err(|e| format!("Failed to open {port_path}: {e}"))?;
        Ok(Self { port })
    }

    pub fn send_and_receive(
        &mut self,
        frame: &[u8],
        opcode: Opcode,
        sequence: u16,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        send_and_receive_io(&mut self.port, frame, opcode, sequence, timeout)
    }
}

pub fn send_and_receive_io<T: Read + Write>(
    io: &mut T,
    request: &[u8],
    expected_opcode: Opcode,
    expected_sequence: u16,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    io.write_all(request)
        .map_err(|e| format!("write failed: {e}"))?;
    io.flush().map_err(|e| format!("flush failed: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut buffered = Vec::new();
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline {
        match io.read(&mut chunk) {
            Ok(0) => std::thread::yield_now(),
            Ok(count) => buffered.extend_from_slice(&chunk[..count]),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(format!("read failed: {error}")),
        }
        while let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
            let frame: Vec<u8> = buffered.drain(..=end).collect();
            if frame.len() > MAX_FRAME_BYTES {
                continue;
            }
            let mut payload = [0u8; MAX_FRAME_BYTES];
            let Ok((header, _)) = decode_frame(&frame, &mut payload) else {
                continue;
            };
            if header.sequence != expected_sequence {
                continue;
            }
            if header.kind != FrameKind::Response {
                return Err("matching frame is not a response".into());
            }
            if header.opcode != expected_opcode {
                return Err(format!("unexpected opcode {:?}", header.opcode));
            }
            if header.status != Status::Ok {
                return Err(format!("device returned {:?}", header.status));
            }
            return Ok(frame);
        }
        if buffered.len() > MAX_FRAME_BYTES {
            if let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
                buffered.drain(..=end);
            } else {
                buffered.clear();
            }
        }
    }
    Err("response timeout without matching sequence".into())
}

pub fn send_batch_and_receive_io<T: Read + Write>(
    io: &mut T,
    requests: &[u8],
    expected_opcode: Opcode,
    expected_sequences: &[u16],
    timeout: Duration,
) -> Result<Vec<Vec<u8>>, String> {
    let observed = send_batch_observed_io(
        io,
        requests,
        expected_opcode,
        expected_sequences,
        timeout,
        0,
    )?;
    expected_sequences
        .iter()
        .map(|sequence| {
            observed
                .iter()
                .find(|response| response.sequence == *sequence)
                .map(|response| response.frame.clone())
                .ok_or_else(|| format!("missing observed sequence {sequence}"))
        })
        .collect()
}

pub fn send_batch_observed_io<T: Read + Write>(
    io: &mut T,
    requests: &[u8],
    expected_opcode: Opcode,
    expected_sequences: &[u16],
    timeout: Duration,
    inter_key_ms: u64,
) -> Result<Vec<ObservedResponse>, String> {
    for (index, &sequence) in expected_sequences.iter().enumerate() {
        if expected_sequences[..index].contains(&sequence) {
            return Err(format!("duplicate expected sequence {sequence}"));
        }
    }

    let started = Instant::now();
    if inter_key_ms == 0 {
        io.write_all(requests)
            .map_err(|e| format!("write failed: {e}"))?;
        io.flush().map_err(|e| format!("flush failed: {e}"))?;
    } else {
        let frames = requests.split_inclusive(|byte| *byte == FRAME_DELIMITER);
        for frame in frames {
            if frame.last() != Some(&FRAME_DELIMITER) {
                return Err("batch contains an incomplete request frame".into());
            }
            io.write_all(frame)
                .map_err(|e| format!("write failed: {e}"))?;
            io.flush().map_err(|e| format!("flush failed: {e}"))?;
            std::thread::sleep(Duration::from_millis(inter_key_ms));
        }
    }

    let deadline = started + timeout;
    let mut buffered = Vec::new();
    let mut chunk = [0u8; 256];
    let mut responses = Vec::with_capacity(expected_sequences.len());

    while Instant::now() < deadline {
        match io.read(&mut chunk) {
            Ok(0) => std::thread::yield_now(),
            Ok(count) => buffered.extend_from_slice(&chunk[..count]),
            Err(error)
                if error.kind() == std::io::ErrorKind::TimedOut
                    || error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(format!("read failed: {error}")),
        }
        while let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
            let frame: Vec<u8> = buffered.drain(..=end).collect();
            if frame.len() > MAX_FRAME_BYTES {
                continue;
            }
            let mut payload = [0u8; MAX_FRAME_BYTES];
            let Ok((header, _)) = decode_frame(&frame, &mut payload) else {
                continue;
            };

            let Some(_) = expected_sequences
                .iter()
                .position(|&sequence| sequence == header.sequence)
            else {
                continue;
            };

            if header.kind != FrameKind::Response {
                return Err("matching frame is not a response".into());
            }
            if header.opcode != expected_opcode {
                return Err(format!("unexpected opcode {:?}", header.opcode));
            }
            if header.status != Status::Ok {
                return Err(format!("device returned {:?}", header.status));
            }
            if responses
                .iter()
                .any(|response: &ObservedResponse| response.sequence == header.sequence)
            {
                return Err(format!("duplicate sequence {}", header.sequence));
            }
            responses.push(ObservedResponse {
                sequence: header.sequence,
                elapsed_ms: started.elapsed().as_millis(),
                frame,
            });
            if responses.len() == expected_sequences.len() {
                return Ok(responses);
            }
        }
        if buffered.len() > MAX_FRAME_BYTES {
            if let Some(end) = buffered.iter().position(|&byte| byte == FRAME_DELIMITER) {
                buffered.drain(..=end);
            } else {
                buffered.clear();
            }
        }
    }

    let missing: Vec<String> = expected_sequences
        .iter()
        .filter(|sequence| {
            !responses
                .iter()
                .any(|response| response.sequence == **sequence)
        })
        .map(u16::to_string)
        .collect();
    if missing.is_empty() {
        Err("response timeout".into())
    } else {
        Err(format!(
            "response timeout missing sequences {}",
            missing.join(",")
        ))
    }
}
