pub const fn update_byte(mut state: u32, byte: u8) -> u32 {
    state ^= byte as u32;
    let mut bit = 0;
    while bit < 8 {
        state = if state & 1 != 0 {
            (state >> 1) ^ 0xedb8_8320
        } else {
            state >> 1
        };
        bit += 1;
    }
    state
}

pub fn iso_hdlc(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    for byte in bytes {
        state = update_byte(state, *byte);
    }
    !state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_iso_hdlc_known_vector() {
        assert_eq!(iso_hdlc(b"123456789"), 0xcbf4_3926);
    }
}
