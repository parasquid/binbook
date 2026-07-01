use gray2_render::{
    canonical_image_to_staged, canonical_row_to_absolute, canonical_row_to_staged, PlaneChunks,
    RenderError,
};

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
fn full_x4_planes_reuse_row_conversion_and_split_into_exact_chunks() {
    let packed = vec![0x1b_u8; 200 * 480];
    let mut msb = vec![0_u8; 100 * 480];
    let mut lsb = vec![0_u8; 100 * 480];
    let mut base = vec![0_u8; 100 * 480];
    canonical_image_to_staged(&packed, 800, 480, &mut msb, &mut lsb, &mut base).unwrap();

    let mut expected_msb = [0_u8; 100];
    let mut expected_lsb = [0_u8; 100];
    let mut expected_base = [0_u8; 100];
    canonical_row_to_staged(
        &packed[..200],
        &mut expected_msb,
        &mut expected_lsb,
        &mut expected_base,
    )
    .unwrap();
    assert_eq!(&msb[..100], &expected_msb);
    assert_eq!(&lsb[..100], &expected_lsb);
    assert_eq!(&base[..100], &expected_base);

    let chunks = PlaneChunks::new(&base, 100, 16)
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(chunks.len(), 30);
    assert!(chunks.iter().all(|chunk| chunk.len() == 1_600));
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
