use binbook_core::CompressionMethod;
use binbook_decompress::decode_exact;
#[cfg(not(feature = "lz4"))]
use binbook_decompress::DecodeError;

#[cfg(not(feature = "lz4"))]
#[test]
fn lz4_is_explicitly_disabled_without_the_feature() {
    assert_eq!(
        decode_exact(CompressionMethod::Lz4, &[], &mut []),
        Err(DecodeError::Lz4Disabled)
    );
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_round_trips_exactly() {
    let input = b"three distinct planes need exact destinations";
    let compressed = lz4_flex::block::compress(input);
    let mut output = [0_u8; 45];
    decode_exact(CompressionMethod::Lz4, &compressed, &mut output).unwrap();
    assert_eq!(&output, input);
}
