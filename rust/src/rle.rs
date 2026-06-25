use crate::Error;

pub fn decompress_packbits(
    input: &[u8],
    out: &mut [u8],
    expected_size: usize,
) -> Result<(), Error> {
    let mut in_pos = 0;
    let mut out_pos = 0;

    while in_pos < input.len() && out_pos < expected_size {
        let control = input[in_pos];
        in_pos += 1;

        if control <= 127 {
            let count = (control as usize) + 1;
            if in_pos + count > input.len() || out_pos + count > expected_size {
                return Err(Error::DecompressFailed);
            }
            out[out_pos..out_pos + count].copy_from_slice(&input[in_pos..in_pos + count]);
            in_pos += count;
            out_pos += count;
        } else if control != 128 {
            let count = ((control & 0x7F) as usize) + 1;
            if in_pos >= input.len() || out_pos + count > expected_size {
                return Err(Error::DecompressFailed);
            }
            let value = input[in_pos];
            in_pos += 1;
            for i in 0..count {
                out[out_pos + i] = value;
            }
            out_pos += count;
        }
    }

    if out_pos != expected_size {
        return Err(Error::DecompressFailed);
    }
    Ok(())
}
