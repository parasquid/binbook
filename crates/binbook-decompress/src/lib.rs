#![no_std]

mod error;
#[cfg(feature = "lz4")]
mod lz4;
mod packbits;

pub use error::DecodeError;
pub use packbits::{DecodeProgress, PackBitsDecoder, PackBitsRun};

use binbook_core::CompressionMethod;

pub fn decode_exact(
    method: CompressionMethod,
    input: &[u8],
    output: &mut [u8],
) -> Result<(), DecodeError> {
    match method {
        CompressionMethod::None => decode_none(input, output),
        CompressionMethod::RlePackBits => packbits::decode_exact(input, output),
        CompressionMethod::Lz4 => decode_lz4(input, output),
    }
}

fn decode_none(input: &[u8], output: &mut [u8]) -> Result<(), DecodeError> {
    match input.len().cmp(&output.len()) {
        core::cmp::Ordering::Less => Err(DecodeError::OutputTooShort {
            expected: output.len(),
            actual: input.len(),
        }),
        core::cmp::Ordering::Greater => Err(DecodeError::OutputTooLong {
            expected: output.len(),
        }),
        core::cmp::Ordering::Equal => {
            output.copy_from_slice(input);
            Ok(())
        }
    }
}

#[cfg(feature = "lz4")]
fn decode_lz4(input: &[u8], output: &mut [u8]) -> Result<(), DecodeError> {
    lz4::decode(input, output)
}

#[cfg(not(feature = "lz4"))]
fn decode_lz4(_input: &[u8], _output: &mut [u8]) -> Result<(), DecodeError> {
    Err(DecodeError::Lz4Disabled)
}
