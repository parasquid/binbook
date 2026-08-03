use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Partition {
    partition_type: String,
    subtype: String,
    offset: u32,
    size: u32,
}

fn parse_hex(value: &str) -> u32 {
    u32::from_str_radix(value.trim_start_matches("0x"), 16).expect("valid hexadecimal field")
}

fn partitions() -> BTreeMap<String, Partition> {
    let csv = include_str!("../../../partitions/xteink-x4.csv");
    csv.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            assert!(fields.len() >= 5, "partition row has five fields: {line}");
            (
                fields[0].to_owned(),
                Partition {
                    partition_type: fields[1].to_owned(),
                    subtype: fields[2].to_owned(),
                    offset: parse_hex(fields[3]),
                    size: parse_hex(fields[4]),
                },
            )
        })
        .collect()
}

#[test]
fn xteink_x4_partition_contract_is_exact_and_bounded() {
    let partitions = partitions();
    assert_eq!(partitions.len(), 6);

    let expected = [
        ("nvs", "data", "nvs", 0x0000_9000, 0x0000_5000),
        ("otadata", "data", "ota", 0x0000_e000, 0x0000_2000),
        ("app0", "app", "ota_0", 0x0001_0000, 0x0064_0000),
        ("app1", "app", "ota_1", 0x0065_0000, 0x0064_0000),
        ("binbook", "data", "littlefs", 0x00c9_0000, 0x0036_0000),
        ("crash", "data", "0x40", 0x00ff_0000, 0x0001_0000),
    ];

    for (name, partition_type, subtype, offset, size) in expected {
        assert_eq!(
            partitions.get(name),
            Some(&Partition {
                partition_type: partition_type.to_owned(),
                subtype: subtype.to_owned(),
                offset,
                size,
            })
        );
    }

    let mut by_offset: Vec<_> = partitions.values().collect();
    by_offset.sort_unstable_by_key(|partition| partition.offset);
    for partition in &by_offset {
        assert_eq!(partition.offset % 0x1000, 0);
        assert_eq!(partition.size % 0x1000, 0);
        assert!(partition.offset.checked_add(partition.size).is_some());
    }
    for pair in by_offset.windows(2) {
        assert!(pair[0].offset + pair[0].size <= pair[1].offset);
    }

    assert_eq!(partitions["app0"].size, partitions["app1"].size);
    assert_eq!(
        partitions["crash"].offset + partitions["crash"].size,
        0x0100_0000
    );
}

#[test]
fn application_slot_fit_check_rejects_one_byte_too_many() {
    assert!(binbook_fw::flash::app_image_fits(
        binbook_fw::flash::APP_SLOT_SIZE
    ));
    assert!(!binbook_fw::flash::app_image_fits(
        binbook_fw::flash::APP_SLOT_SIZE + 1
    ));
}

#[test]
fn firmware_flash_constants_match_the_partition_contract() {
    assert_eq!(binbook_fw::flash::LITTLEFS_OFFSET, 0x00c9_0000);
    assert_eq!(binbook_fw::flash::LITTLEFS_SIZE, 0x0036_0000);
    assert_eq!(binbook_fw::flash::CRASH_SECTOR_OFFSET, 0x00ff_0000);
    assert_eq!(binbook_fw::flash::CRASH_SECTOR_SIZE, 0x1000);
}
