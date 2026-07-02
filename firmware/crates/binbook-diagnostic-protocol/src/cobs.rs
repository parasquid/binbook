use crate::{ProtocolError, FRAME_DELIMITER};

pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

pub fn cobs_encode(input: &[u8], output: &mut [u8]) -> Result<usize, ProtocolError> {
    if output.len() < input.len() + 2 {
        return Err(ProtocolError::OutputTooSmall);
    }

    let mut read_pos = 0;
    let mut write_pos = 1;
    let mut code_pos = 0;
    let mut code = 1u8;

    while read_pos < input.len() {
        if input[read_pos] == 0 {
            output[code_pos] = code;
            code_pos = write_pos;
            write_pos += 1;
            code = 1;
        } else {
            if code == 0xFF {
                output[code_pos] = code;
                code_pos = write_pos;
                write_pos += 1;
                code = 1;
            }
            output[write_pos] = input[read_pos];
            write_pos += 1;
            code += 1;
        }
        read_pos += 1;
    }

    output[code_pos] = code;
    output[write_pos] = FRAME_DELIMITER;
    write_pos += 1;

    Ok(write_pos)
}

pub fn cobs_decode(input: &[u8], output: &mut [u8]) -> Result<usize, ProtocolError> {
    if input.is_empty() || input[input.len() - 1] != FRAME_DELIMITER {
        return Err(ProtocolError::BadCobs);
    }

    let mut read_pos = 0;
    let mut write_pos = 0;

    loop {
        if read_pos >= input.len() - 1 {
            break;
        }

        let code = input[read_pos] as usize;
        read_pos += 1;

        if code == 0 {
            return Err(ProtocolError::BadCobs);
        }

        let end = read_pos + code - 1;
        if end > input.len() - 1 {
            return Err(ProtocolError::BadCobs);
        }

        while read_pos < end {
            if write_pos >= output.len() {
                return Err(ProtocolError::OutputTooSmall);
            }
            output[write_pos] = input[read_pos];
            write_pos += 1;
            read_pos += 1;
        }

        if code < 0xFF && read_pos < input.len() - 1 {
            if write_pos >= output.len() {
                return Err(ProtocolError::OutputTooSmall);
            }
            output[write_pos] = 0;
            write_pos += 1;
        }
    }

    Ok(write_pos)
}
