use crate::Error;

pub fn decompress_lz4(input: &[u8], out: &mut [u8], expected_size: usize) -> Result<(), Error> {
    let decompressed =
        lz4_flex::decompress(input, expected_size).map_err(|_| Error::DecompressFailed)?;
    if decompressed.len() != expected_size {
        return Err(Error::DecompressFailed);
    }
    if out.len() < expected_size {
        return Err(Error::OutputBufferTooSmall);
    }
    out[..expected_size].copy_from_slice(&decompressed);
    Ok(())
}
