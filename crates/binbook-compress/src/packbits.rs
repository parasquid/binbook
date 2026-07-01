#[cfg(feature = "alloc")]
use alloc::vec;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

const MAX_RUN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    BufferTooSmall { required: usize, provided: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Literal,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    kind: RunKind,
    length: usize,
}

#[must_use]
pub fn encoded_len(input: &[u8]) -> usize {
    let mut input_offset = 0;
    let mut output_length = 0_usize;
    while input_offset < input.len() {
        let run = next_run(input, input_offset);
        let payload = match run.kind {
            RunKind::Literal => run.length,
            RunKind::Repeat => 1,
        };
        output_length = output_length.saturating_add(1 + payload);
        input_offset += run.length;
    }
    output_length
}

pub fn encode_into(input: &[u8], output: &mut [u8]) -> Result<usize, EncodeError> {
    let required = encoded_len(input);
    if output.len() < required {
        return Err(EncodeError::BufferTooSmall {
            required,
            provided: output.len(),
        });
    }
    Ok(encode_runs(input, output))
}

fn encode_runs(input: &[u8], output: &mut [u8]) -> usize {
    let mut input_offset = 0;
    let mut output_offset = 0;
    while input_offset < input.len() {
        let run = next_run(input, input_offset);
        match run.kind {
            RunKind::Literal => {
                output[output_offset] = (run.length - 1) as u8;
                output_offset += 1;
                output[output_offset..output_offset + run.length]
                    .copy_from_slice(&input[input_offset..input_offset + run.length]);
                output_offset += run.length;
            }
            RunKind::Repeat => {
                output[output_offset] = 0x80 | (run.length - 1) as u8;
                output[output_offset + 1] = input[input_offset];
                output_offset += 2;
            }
        }
        input_offset += run.length;
    }
    output_offset
}

#[cfg(feature = "alloc")]
#[must_use]
pub fn encode(input: &[u8]) -> Vec<u8> {
    let mut output = vec![0_u8; encoded_len(input)];
    encode_runs(input, &mut output);
    output
}

fn next_run(input: &[u8], offset: usize) -> Run {
    let repeat = repeat_length(input, offset);
    if repeat >= 2 {
        return Run {
            kind: RunKind::Repeat,
            length: repeat,
        };
    }
    let mut length = 1;
    while length < MAX_RUN && offset + length < input.len() {
        if repeat_length(input, offset + length) >= 2 {
            break;
        }
        length += 1;
    }
    Run {
        kind: RunKind::Literal,
        length,
    }
}

fn repeat_length(input: &[u8], offset: usize) -> usize {
    let Some(&value) = input.get(offset) else {
        return 0;
    };
    let available = input.len() - offset;
    let mut length = 1;
    while length < MAX_RUN && length < available && input[offset + length] == value {
        length += 1;
    }
    length
}
