use gray2_render::{staged_byte_to_absolute, AbsolutePlaneByte};

#[test]
fn staged_reconstruction_matches_format_formulas() {
    let result = staged_byte_to_absolute(0b0110_0110, 0b0100_0100, 0b0001_0001);
    assert_eq!(
        result,
        AbsolutePlaneByte {
            red: !(0b0001_0001 | (0b0110_0110 & !0b0100_0100)),
            black: !(0b0001_0001 | 0b0100_0100),
        }
    );
}

#[test]
fn black_white_and_gray_levels_remain_distinct() {
    let result = staged_byte_to_absolute(0x60, 0x40, 0x1f);
    assert_ne!(result.red, result.black);
    assert_ne!(result.red, 0);
    assert_ne!(result.black, 0);
}
