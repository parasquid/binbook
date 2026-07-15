use std::io::{Read, Write};
use std::time::{Duration, Instant};

use binbook_diagnostic_protocol::{decode_frame, FrameKind, FRAME_DELIMITER, MAX_FRAME_BYTES};

pub(crate) fn exchange_batch<T: Read + Write>(
    io: &mut T,
    requests: &[u8],
    expected_sequences: &[u16],
    timeout: Duration,
) -> Result<Vec<Vec<u8>>, String> {
    io.write_all(requests)
        .map_err(|error| format!("write failed: {error}"))?;
    io.flush()
        .map_err(|error| format!("flush failed: {error}"))?;

    let deadline = Instant::now() + timeout;
    let mut buffered = Vec::new();
    let mut responses = Vec::with_capacity(expected_sequences.len());
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline && responses.len() < expected_sequences.len() {
        match io.read(&mut chunk) {
            Ok(0) => std::thread::yield_now(),
            Ok(count) => buffered.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(format!("read failed: {error}")),
        }
        drain_matching_frames(&mut buffered, expected_sequences, &mut responses)?;
        if buffered.len() > MAX_FRAME_BYTES {
            buffered.clear();
        }
    }
    expected_sequences
        .iter()
        .map(|sequence| {
            responses
                .iter()
                .find(|(observed, _)| observed == sequence)
                .map(|(_, frame)| frame.clone())
                .ok_or_else(|| format!("response timeout missing sequence {sequence}"))
        })
        .collect()
}

fn drain_matching_frames(
    buffered: &mut Vec<u8>,
    expected_sequences: &[u16],
    responses: &mut Vec<(u16, Vec<u8>)>,
) -> Result<(), String> {
    while let Some(end) = buffered.iter().position(|byte| *byte == FRAME_DELIMITER) {
        let frame: Vec<u8> = buffered.drain(..=end).collect();
        if frame.len() > MAX_FRAME_BYTES {
            continue;
        }
        let mut payload = [0u8; MAX_FRAME_BYTES];
        let Ok((header, _)) = decode_frame(&frame, &mut payload) else {
            continue;
        };
        if !expected_sequences.contains(&header.sequence) {
            continue;
        }
        if header.kind != FrameKind::Response {
            return Err(format!(
                "matching frame {} is not a response",
                header.sequence
            ));
        }
        if responses
            .iter()
            .any(|(sequence, _)| *sequence == header.sequence)
        {
            return Err(format!("duplicate sequence {}", header.sequence));
        }
        responses.push((header.sequence, frame));
    }
    Ok(())
}
