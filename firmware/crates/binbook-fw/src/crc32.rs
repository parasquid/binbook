const TABLE: [u32; 256] = make_table();

const fn make_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xedb8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

pub fn update_byte(state: u32, byte: u8) -> u32 {
    TABLE[((state ^ u32::from(byte)) & 0xff) as usize] ^ (state >> 8)
}

pub fn update(mut state: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        state = update_byte(state, *byte);
    }
    state
}

pub fn iso_hdlc(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    state = update(state, bytes);
    !state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_iso_hdlc_known_vector() {
        assert_eq!(iso_hdlc(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn table_matches_bitwise_reference_for_every_byte_and_chunking() {
        fn reference(mut state: u32, byte: u8) -> u32 {
            state ^= u32::from(byte);
            for _ in 0..8 {
                state = if state & 1 != 0 {
                    (state >> 1) ^ 0xedb8_8320
                } else {
                    state >> 1
                };
            }
            state
        }

        for byte in u8::MIN..=u8::MAX {
            assert_eq!(update_byte(0xa5a5_5a5a, byte), reference(0xa5a5_5a5a, byte));
        }
        let bytes: [u8; 1024] = core::array::from_fn(|index| index as u8);
        assert_eq!(
            update(update(u32::MAX, &bytes[..333]), &bytes[333..]),
            update(u32::MAX, &bytes)
        );
    }
}
