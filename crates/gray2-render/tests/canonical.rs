use gray2_render::{canonical_bits, CanonicalGray2, PlaneBits};

#[test]
fn canonical_levels_map_to_absolute_ssd1677_planes() {
    let cases = [
        (CanonicalGray2::Black, PlaneBits::new(true, true)),
        (CanonicalGray2::DarkGray, PlaneBits::new(true, false)),
        (CanonicalGray2::LightGray, PlaneBits::new(false, true)),
        (CanonicalGray2::White, PlaneBits::new(false, false)),
    ];
    for (gray, expected) in cases {
        assert_eq!(canonical_bits(gray), expected);
    }
}

#[test]
fn every_packed_byte_matches_per_pixel_conversion() {
    for packed in 0_u8..=u8::MAX {
        let pixels = gray2_render::unpack(packed);
        for (index, gray) in pixels.into_iter().enumerate() {
            assert_eq!(u8::from(gray), (packed >> (6 - index * 2)) & 3);
        }
    }
}
