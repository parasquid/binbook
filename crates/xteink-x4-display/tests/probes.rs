use xteink_x4_display::probes::{
    build_corner_row, corner_windows, DISPLAY_HEIGHT, DISPLAY_ROW_BYTES, DISPLAY_WIDTH,
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
}
