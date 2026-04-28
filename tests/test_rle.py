from binbook.rle import decode_packbits, encode_packbits


def test_rle_roundtrips_literal_and_repeated_runs():
    data = b"abc" + bytes([7]) * 140 + b"xyz"
    assert decode_packbits(encode_packbits(data)) == data


def test_control_0x80_is_one_byte_repeat_not_noop():
    assert decode_packbits(bytes([0x80, 0xAA])) == bytes([0xAA])
