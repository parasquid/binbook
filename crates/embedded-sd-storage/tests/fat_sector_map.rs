use embedded_sd_storage::{FatGeometry, SectorPurpose, SemanticSectorMap};

fn fat32_boot() -> [u8; 512] {
    let mut boot = [0u8; 512];
    boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot[13] = 8;
    boot[14..16].copy_from_slice(&32u16.to_le_bytes());
    boot[16] = 2;
    boot[32..36].copy_from_slice(&1_000_000u32.to_le_bytes());
    boot[36..40].copy_from_slice(&1_024u32.to_le_bytes());
    boot[44..48].copy_from_slice(&2u32.to_le_bytes());
    boot[510..512].copy_from_slice(&0xaa55u16.to_le_bytes());
    boot
}

#[test]
fn fat32_geometry_classifies_reserved_each_fat_copy_and_data_boundaries() {
    let geometry = FatGeometry::from_boot_sector(2_048, &fat32_boot()).unwrap();
    assert_eq!(geometry.sectors_per_cluster(), 8);
    assert_eq!(geometry.root_cluster(), 2);
    assert_eq!(geometry.classify(2_048), SectorPurpose::Reserved);
    assert_eq!(geometry.classify(2_079), SectorPurpose::Reserved);
    assert_eq!(geometry.classify(2_080), SectorPurpose::Fat { copy: 0 });
    assert_eq!(geometry.classify(3_103), SectorPurpose::Fat { copy: 0 });
    assert_eq!(geometry.classify(3_104), SectorPurpose::Fat { copy: 1 });
    assert_eq!(geometry.classify(4_127), SectorPurpose::Fat { copy: 1 });
    assert_eq!(geometry.classify(4_128), SectorPurpose::DataOther);
}

#[test]
fn bounded_semantic_ranges_override_only_data_sectors_with_locked_precedence() {
    let geometry = FatGeometry::from_boot_sector(2_048, &fat32_boot()).unwrap();
    let mut map = SemanticSectorMap::<4>::new(geometry);
    map.add_range(4_128, 8, SectorPurpose::Directory).unwrap();
    map.add_range(4_136, 2, SectorPurpose::Journal).unwrap();
    map.add_range(5_000, 16, SectorPurpose::Payload).unwrap();

    assert_eq!(map.classify(4_128), SectorPurpose::Directory);
    assert_eq!(map.classify(4_137), SectorPurpose::Journal);
    assert_eq!(map.classify(5_015), SectorPurpose::Payload);
    assert_eq!(map.classify(5_016), SectorPurpose::DataOther);
    assert_eq!(map.classify(3_104), SectorPurpose::Fat { copy: 1 });
    assert!(map.add_range(2_080, 1, SectorPurpose::Payload).is_err());
    assert!(map.add_range(6_000, 0, SectorPurpose::Payload).is_err());
}

#[test]
fn repeated_exact_semantic_registration_is_idempotent_and_preserves_capacity() {
    let geometry = FatGeometry::from_boot_sector(2_048, &fat32_boot()).unwrap();
    let mut map = SemanticSectorMap::<1>::new(geometry);

    map.add_range(4_136, 1, SectorPurpose::Journal).unwrap();
    map.add_range(4_136, 1, SectorPurpose::Journal).unwrap();

    assert_eq!(map.classify(4_136), SectorPurpose::Journal);
    assert_eq!(
        map.add_range(4_137, 1, SectorPurpose::Journal),
        Err(embedded_sd_storage::FatGeometryError::Capacity)
    );
}

#[test]
fn malformed_bpb_is_rejected_instead_of_producing_plausible_ranges() {
    for mutate in [
        |boot: &mut [u8; 512]| boot[11..13].copy_from_slice(&1_024u16.to_le_bytes()),
        |boot: &mut [u8; 512]| boot[13] = 3,
        |boot: &mut [u8; 512]| boot[16] = 0,
        |boot: &mut [u8; 512]| boot[36..40].fill(0),
        |boot: &mut [u8; 512]| boot[510..512].fill(0),
    ] {
        let mut boot = fat32_boot();
        mutate(&mut boot);
        assert!(FatGeometry::from_boot_sector(2_048, &boot).is_err());
    }
}
