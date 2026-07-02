use xteink_x4_display::probes::{
    build_corner_row, corner_windows, partial_window_busy_cases, probe_polarity,
    PartialWindowPolarity, DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH,
};

#[test]
fn corner_probe_covers_each_physical_corner() {
    let windows = corner_windows();
    assert_eq!(windows[0].0, 0);
    assert_eq!(windows[0].1, 0);
    assert_eq!(windows[3].0 + windows[3].2, DISPLAY_WIDTH);
    assert_eq!(windows[3].1 + windows[3].3, DISPLAY_HEIGHT);
    let mut row = [0_u8; DISPLAY_ROW_BYTES];
    build_corner_row(0, &mut row);
    assert_eq!(&row[..16], &[0; 16]);
    assert_eq!(&row[84..], &[0; 16]);
}

#[test]
fn probe_kinds_include_full_refresh_and_clear() {
    use xteink_x4_display::probes::ProbeKind;
    assert_ne!(ProbeKind::FullRefreshCurrent, ProbeKind::ClearWhite);
    assert_ne!(ProbeKind::ClearWhite, ProbeKind::WindowCorners);
    assert_ne!(ProbeKind::WindowCorners, ProbeKind::PartialWindowBusyMatrix);
}

#[test]
fn partial_window_busy_matrix_uses_stable_geometries_and_byte_counts() {
    let cases = partial_window_busy_cases();
    assert_eq!(cases.len(), 6);
    assert_eq!(cases[0].name, "full");
    assert_eq!((cases[0].x, cases[0].y, cases[0].width, cases[0].height), (0, 0, 800, 480));
    assert_eq!(cases[0].bytes_per_plane(), 48_000);
    assert_eq!(cases[1].name, "half-height");
    assert_eq!((cases[1].x, cases[1].y, cases[1].width, cases[1].height), (0, 120, 800, 240));
    assert_eq!(cases[1].bytes_per_plane(), 24_000);
    assert_eq!(cases[2].name, "stripe-64");
    assert_eq!((cases[2].x, cases[2].y, cases[2].width, cases[2].height), (0, 208, 800, 64));
    assert_eq!(cases[2].bytes_per_plane(), 6_400);
    assert_eq!(cases[3].name, "strip-16");
    assert_eq!((cases[3].x, cases[3].y, cases[3].width, cases[3].height), (0, 232, 800, 16));
    assert_eq!(cases[3].bytes_per_plane(), 1_600);
    assert_eq!(cases[4].name, "center-box");
    assert_eq!((cases[4].x, cases[4].y, cases[4].width, cases[4].height), (300, 190, 200, 100));
    assert_eq!(cases[4].bytes_per_plane(), 2_500);
    assert_eq!(cases[5].name, "page-number-box");
    assert_eq!((cases[5].x, cases[5].y, cases[5].width, cases[5].height), (280, 0, 240, 80));
    assert_eq!(cases[5].bytes_per_plane(), 2_400);
}

#[test]
fn partial_window_probe_alternates_real_pixel_transitions() {
    assert_eq!(probe_polarity(1), PartialWindowPolarity::BlackOnWhite);
    assert_eq!(probe_polarity(2), PartialWindowPolarity::WhiteOnBlack);
    assert_eq!(probe_polarity(3), PartialWindowPolarity::BlackOnWhite);
}
