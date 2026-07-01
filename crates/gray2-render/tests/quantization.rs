use gray2_render::{
    pack_gray1_row, pack_gray2_row, quantize_gray1, quantize_gray2, FloydSteinberg, RenderError,
};

#[test]
fn gray1_and_gray2_thresholds_match_python() {
    assert_eq!(
        (0_u8..=255).map(quantize_gray1).collect::<Vec<_>>(),
        (0_u8..=255)
            .map(|value| u8::from(value >= 128))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        [0, 42, 43, 127, 128, 212, 213, 255].map(quantize_gray2),
        [0, 0, 1, 1, 2, 2, 3, 3]
    );
}

#[test]
fn floyd_steinberg_rows_match_python_and_use_only_row_scratch() {
    let gray1_input = [64, 128, 192, 32, 96, 160, 224, 16];
    let mut gray1_current = [0_f32; 6];
    let mut gray1_next = [0_f32; 6];
    let mut gray1 = FloydSteinberg::new(4, &mut gray1_current, &mut gray1_next).unwrap();
    let mut gray1_output = [0_u8; 8];
    for row in 0..2 {
        gray1
            .quantize_gray1_row(
                &gray1_input[row * 4..row * 4 + 4],
                &mut gray1_output[row * 4..row * 4 + 4],
            )
            .unwrap();
    }
    assert_eq!(gray1_output, [0, 1, 1, 0, 0, 1, 1, 0]);

    let input = [120_u8; 12];
    let mut current = [0_f32; 6];
    let mut next = [0_f32; 6];
    let mut state = FloydSteinberg::new(4, &mut current, &mut next).unwrap();
    let mut actual = [0_u8; 12];
    for row in 0..3 {
        state
            .quantize_gray2_row(
                &input[row * 4..row * 4 + 4],
                &mut actual[row * 4..row * 4 + 4],
            )
            .unwrap();
    }
    assert_eq!(actual, [1, 2, 1, 2, 1, 2, 1, 1, 1, 2, 1, 2]);

    let width = 257;
    let mut current = vec![0_f32; width + 2];
    let mut next = vec![0_f32; width + 2];
    let mut state = FloydSteinberg::new(width, &mut current, &mut next).unwrap();
    let row = vec![127_u8; width];
    let mut output = vec![0_u8; width];
    for _ in 0..5 {
        state.quantize_gray1_row(&row, &mut output).unwrap();
    }
    assert!(output.iter().all(|value| *value <= 1));
}

#[test]
fn packing_is_msb_first_and_reports_exact_buffer_requirements() {
    let mut gray2 = [0_u8; 2];
    pack_gray2_row(&[0, 1, 2, 3, 3], &mut gray2).unwrap();
    assert_eq!(gray2, [0x1b, 0xc0]);
    let mut gray1 = [0_u8; 2];
    pack_gray1_row(&[0, 1, 0, 1, 1, 0, 1, 0, 1], &mut gray1).unwrap();
    assert_eq!(gray1, [0x5a, 0x80]);
    assert_eq!(
        pack_gray2_row(&[0; 5], &mut [0_u8; 1]),
        Err(RenderError::BufferTooSmall {
            required: 2,
            provided: 1
        })
    );
    assert_eq!(
        FloydSteinberg::new(8, &mut [0_f32; 9], &mut [0_f32; 10]),
        Err(RenderError::BufferTooSmall {
            required: 10,
            provided: 9
        })
    );
}
