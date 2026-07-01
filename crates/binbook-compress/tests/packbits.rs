use binbook_compress::{encode, encode_into, encoded_len, EncodeError};
use binbook_core::CompressionMethod;
use binbook_decompress::decode_exact;

fn assert_encoding(input: &[u8], expected: &[u8]) {
    assert_eq!(encoded_len(input), expected.len());
    assert_eq!(encode(input), expected);
    let mut output = vec![0xaa; expected.len()];
    let written = encode_into(input, &mut output).expect("exact output capacity");
    assert_eq!(written, expected.len());
    assert_eq!(output, expected);
    assert_roundtrip(input, expected);
}

fn assert_roundtrip(input: &[u8], encoded: &[u8]) {
    let mut decoded = vec![0_u8; input.len()];
    decode_exact(CompressionMethod::RlePackBits, encoded, &mut decoded).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn golden_empty_repeat_and_literal_boundaries() {
    assert_encoding(&[], &[]);
    assert_encoding(&[0x2a], &[0, 0x2a]);
    assert_encoding(&[0x2a; 2], &[0x81, 0x2a]);
    assert_encoding(&[0x2a; 127], &[0xfe, 0x2a]);
    assert_encoding(&[0x2a; 128], &[0xff, 0x2a]);
    assert_encoding(&[0x2a; 129], &[0xff, 0x2a, 0, 0x2a]);

    for length in [127_usize, 128, 129] {
        let input: Vec<u8> = (0..length).map(|value| value as u8).collect();
        let encoded = encode(&input);
        let first_literal = length.min(128);
        assert_eq!(encoded[0], (first_literal - 1) as u8);
        assert_roundtrip(&input, &encoded);
    }
}

#[test]
fn deterministic_split_runs_and_alternating_bytes() {
    assert_encoding(&[1, 1, 2, 3, 3], &[0x81, 1, 0, 2, 0x81, 3]);
    let alternating: Vec<u8> = (0..513).map(|index| (index & 1) as u8).collect();
    let first = encode(&alternating);
    assert_eq!(first, encode(&alternating));
    assert_roundtrip(&alternating, &first);
}

#[test]
fn binbook_one_byte_repeat_control_decodes_as_one_byte() {
    let mut output = [0_u8; 1];
    decode_exact(CompressionMethod::RlePackBits, &[0x80, 0x5a], &mut output).unwrap();
    assert_eq!(output, [0x5a]);
}

#[test]
fn reports_exact_output_capacity_without_partial_writes() {
    let input = [7_u8; 128];
    let mut output = [0xaa; 1];
    assert_eq!(
        encode_into(&input, &mut output),
        Err(EncodeError::BufferTooSmall {
            required: 2,
            provided: 1,
        })
    );
    assert_eq!(output, [0xaa]);
}

#[test]
fn generated_patterns_and_large_inputs_roundtrip_through_firmware_decoder() {
    for length in 0..=512 {
        let input: Vec<u8> = (0..length)
            .map(|index| ((index * 37 + length * 13) ^ (index >> 2)) as u8)
            .collect();
        assert_roundtrip(&input, &encode(&input));
    }

    let mut large = Vec::with_capacity(9_217);
    for index in 0..9_217 {
        large.push(if index % 257 < 96 { 0xe5 } else { index as u8 });
    }
    assert!(large.len() > 8 * 1024);
    assert_roundtrip(&large, &encode(&large));
}
