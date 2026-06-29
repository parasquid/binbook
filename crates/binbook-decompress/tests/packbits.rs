use binbook_core::CompressionMethod;
use binbook_decompress::{decode_exact, DecodeError};

#[test]
fn decodes_literal_repeat_and_one_byte_repeat_runs() {
    let mut output = [0_u8; 6];
    decode_exact(
        CompressionMethod::RlePackBits,
        &[1, 0x10, 0x20, 0x80, 0x30, 0x82, 0x7f],
        &mut output,
    )
    .unwrap();
    assert_eq!(output, [0x10, 0x20, 0x30, 0x7f, 0x7f, 0x7f]);
}

#[test]
fn rejects_malformed_and_inexact_output() {
    let mut output = [0_u8; 2];
    assert_eq!(
        decode_exact(CompressionMethod::RlePackBits, &[2, 1, 2], &mut output),
        Err(DecodeError::MalformedRun)
    );
    assert_eq!(
        decode_exact(CompressionMethod::RlePackBits, &[0, 1], &mut [0_u8; 2]),
        Err(DecodeError::OutputTooShort {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(
        decode_exact(CompressionMethod::RlePackBits, &[1, 1, 2], &mut [0_u8; 1]),
        Err(DecodeError::OutputTooLong { expected: 1 })
    );
}

#[test]
fn none_is_also_exact() {
    assert_eq!(
        decode_exact(CompressionMethod::None, &[1], &mut [0_u8; 2]),
        Err(DecodeError::OutputTooShort {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(
        decode_exact(CompressionMethod::None, &[1, 2], &mut [0_u8; 1]),
        Err(DecodeError::OutputTooLong { expected: 1 })
    );
}
