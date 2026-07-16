use crate::{ProtocolError, FRAME_DELIMITER};

const CRC16_TABLE: [u16; 256] = make_crc16_table();

const fn make_crc16_table() -> [u16; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = (index as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000 != 0 {
                (value << 1) ^ 0x1021
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc = (crc << 8) ^ CRC16_TABLE[usize::from((crc >> 8) ^ u16::from(byte))];
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

pub fn cobs_decode_in_place(buffer: &mut [u8], input_len: usize) -> Result<usize, ProtocolError> {
    if input_len == 0 || input_len > buffer.len() || buffer[input_len - 1] != FRAME_DELIMITER {
        return Err(ProtocolError::BadCobs);
    }

    let mut read_pos = 0;
    let mut write_pos = 0;
    while read_pos < input_len - 1 {
        let code = usize::from(buffer[read_pos]);
        read_pos += 1;
        if code == 0 {
            return Err(ProtocolError::BadCobs);
        }
        let end = read_pos + code - 1;
        if end > input_len - 1 {
            return Err(ProtocolError::BadCobs);
        }
        while read_pos < end {
            buffer[write_pos] = buffer[read_pos];
            write_pos += 1;
            read_pos += 1;
        }
        if code < 0xff && read_pos < input_len - 1 {
            buffer[write_pos] = 0;
            write_pos += 1;
        }
    }
    Ok(write_pos)
}
