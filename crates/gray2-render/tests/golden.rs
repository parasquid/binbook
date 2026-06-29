use gray2_render::{canonical_row_to_absolute, canonical_row_to_staged, RenderError};

#[test]
fn staged_row_matches_python_writer_golden() {
    let mut input = [0xff_u8; 200];
    input[0] = 0x1b;
    input[199] = 0x6c;
    let mut msb = [0_u8; 100];
    let mut lsb = [0_u8; 100];
    let mut base = [0_u8; 100];
    canonical_row_to_staged(&input, &mut msb, &mut lsb, &mut base).unwrap();

    assert_eq!(msb[0], 0x30);
    assert_eq!(lsb[0], 0x10);
    assert_eq!(base[0], 0x4f);
    assert_eq!(msb[99], 0x06);
    assert_eq!(lsb[99], 0x02);
    assert_eq!(base[99], 0xf8);
    assert!(msb[1..99].iter().all(|byte| *byte == 0));
    assert!(base[1..99].iter().all(|byte| *byte == 0xff));
}

#[test]
fn absolute_row_reports_exact_output_size() {
    let input = [0_u8; 4];
    assert_eq!(
        canonical_row_to_absolute(&input, &mut [0_u8; 1], &mut [0_u8; 2]),
        Err(RenderError::BufferTooSmall {
            required: 2,
            provided: 1,
        })
    );
}
