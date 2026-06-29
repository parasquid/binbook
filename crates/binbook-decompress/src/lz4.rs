use crate::DecodeError;

pub(crate) fn decode(input: &[u8], output: &mut [u8]) -> Result<(), DecodeError> {
    let written =
        lz4_flex::block::decompress_into(input, output).map_err(|_| DecodeError::Lz4Failure)?;
    match written.cmp(&output.len()) {
        core::cmp::Ordering::Less => Err(DecodeError::OutputTooShort {
            expected: output.len(),
            actual: written,
        }),
        core::cmp::Ordering::Greater => Err(DecodeError::OutputTooLong {
            expected: output.len(),
        }),
        core::cmp::Ordering::Equal => Ok(()),
    }
}
