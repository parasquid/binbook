use binbook_core::{
    ByteLength, CompressionMethod, DisplayProfile, FileOffset, PageInfo, PageNumber, PixelFormat,
    PlaneDescriptor, PlaneDirectory, PlaneSlot, StringRef, WAVEFORM_SSD1677_STAGED_GRAY2,
};
use xteink_x4_display::profile::{
    logical_gray2_to_physical_packed, logical_to_physical, validate_page, validate_profile,
    CHUNK_ROWS, LOGICAL_HEIGHT, LOGICAL_WIDTH, PHYSICAL_HEIGHT, PHYSICAL_WIDTH,
};

fn profile() -> DisplayProfile {
    DisplayProfile {
        profile_id: StringRef {
            offset: 0,
            length: 0,
        },
        device_family: StringRef {
            offset: 0,
            length: 0,
        },
        device_model: StringRef {
            offset: 0,
            length: 0,
        },
        logical_width: LOGICAL_WIDTH,
        logical_height: LOGICAL_HEIGHT,
        physical_width: PHYSICAL_WIDTH,
        physical_height: PHYSICAL_HEIGHT,
        logical_orientation: 1,
        logical_to_physical_rotation: 270,
        scan_order_hint: 0,
        supported_storage_pixel_formats: 0,
        native_output_pixel_formats: 0,
        native_grayscale_levels: 4,
        panel_grayscale_levels: 4,
        framebuffer_bits_per_pixel: 1,
        waveform_hint: WAVEFORM_SSD1677_STAGED_GRAY2,
        dither_mode: 0,
    }
}

#[test]
fn logical_gray2_corners_pack_through_the_shared_x4_mapping() {
    let mut logical = vec![3_u8; usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT)];
    logical[0] = 0;
    logical[usize::from(LOGICAL_WIDTH) - 1] = 1;
    logical[(usize::from(LOGICAL_HEIGHT) - 1) * usize::from(LOGICAL_WIDTH)] = 2;
    let mut packed = vec![0_u8; usize::from(PHYSICAL_WIDTH) * usize::from(PHYSICAL_HEIGHT) / 4];
    logical_gray2_to_physical_packed(&logical, &mut packed).unwrap();
    for (logical_x, logical_y, expected) in [
        (0, 0, 0),
        (LOGICAL_WIDTH - 1, 0, 1),
        (0, LOGICAL_HEIGHT - 1, 2),
        (LOGICAL_WIDTH - 1, LOGICAL_HEIGHT - 1, 3),
    ] {
        let (x, y) = logical_to_physical(logical_x, logical_y);
        let index = usize::from(y) * usize::from(PHYSICAL_WIDTH) + usize::from(x);
        assert_eq!((packed[index / 4] >> (6 - (index % 4) * 2)) & 3, expected);
    }
}

fn plane(slot: PlaneSlot) -> PlaneDescriptor {
    PlaneDescriptor::new(
        slot,
        CompressionMethod::RlePackBits,
        FileOffset::new(match slot {
            PlaneSlot::OverlayMsb => 0,
            PlaneSlot::OverlayLsb => 10,
            PlaneSlot::FastBase => 20,
            PlaneSlot::Reserved => 30,
        }),
        ByteLength::new(10),
    )
}

fn page() -> PageInfo {
    PageInfo {
        page_number: PageNumber::new(0, 1).unwrap(),
        page_kind: 0,
        pixel_format: PixelFormat::Gray2Packed,
        compression_method: CompressionMethod::RlePackBits,
        update_hint: WAVEFORM_SSD1677_STAGED_GRAY2,
        page_flags: 0,
        page_crc32: 0,
        stored_width: PHYSICAL_WIDTH,
        stored_height: PHYSICAL_HEIGHT,
        placement_x: 0,
        placement_y: 0,
        progress_start_ppm: 0,
        progress_end_ppm: 1_000_000,
        planes: PlaneDirectory::new([
            Some(plane(PlaneSlot::OverlayMsb)),
            Some(plane(PlaneSlot::OverlayLsb)),
            Some(plane(PlaneSlot::FastBase)),
            None,
        ]),
    }
}

#[test]
fn x4_profile_and_rotation_are_exact() {
    assert_eq!(CHUNK_ROWS, 16);
    assert_eq!(logical_to_physical(0, 0), (799, 0));
    assert_eq!(logical_to_physical(479, 799), (0, 479));
    assert_eq!(validate_profile(&profile()), Ok(()));
}

#[test]
fn x4_page_requires_three_staged_planes_and_exact_geometry() {
    assert_eq!(validate_page(&page()), Ok(()));
    let mut invalid = page();
    invalid.stored_height -= 1;
    assert!(validate_page(&invalid).is_err());
    invalid = page();
    invalid.planes = PlaneDirectory::new([Some(plane(PlaneSlot::OverlayMsb)), None, None, None]);
    assert!(validate_page(&invalid).is_err());
}
