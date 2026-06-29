use binbook_decompress::PackBitsDecoder;

#[test]
fn literal_and_repeat_runs_cross_input_and_output_boundaries() {
    let mut decoder = PackBitsDecoder::new();
    let mut first = [0_u8; 2];
    let progress = decoder.decode(&[3, 1, 2], &mut first).unwrap();
    assert_eq!((progress.consumed, progress.produced), (3, 2));
    assert_eq!(first, [1, 2]);

    let mut second = [0_u8; 3];
    let progress = decoder.decode(&[3, 4, 0x84, 9], &mut second).unwrap();
    assert_eq!((progress.consumed, progress.produced), (4, 3));
    assert_eq!(second, [3, 4, 9]);

    let mut third = [0_u8; 4];
    let progress = decoder.decode(&[], &mut third).unwrap();
    assert_eq!((progress.consumed, progress.produced), (0, 4));
    assert_eq!(third, [9; 4]);
    assert!(decoder.is_idle());
}

#[test]
fn independent_planes_keep_independent_decoder_state() {
    let encoded = [[0xfe, 0x11], [0xfe, 0x22], [0xfe, 0x33]];
    let mut outputs = [[0_u8; 3]; 3];
    for (input, output) in encoded.iter().zip(outputs.iter_mut()) {
        let mut decoder = PackBitsDecoder::new();
        let progress = decoder.decode(input, output).unwrap();
        assert_eq!(progress.produced, 3);
    }
    assert_eq!(outputs, [[0x11; 3], [0x22; 3], [0x33; 3]]);
}
