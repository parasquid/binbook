use binbook_core::{validate_all, SliceSource, ValidationCode, ValidationIssue, ValidationVisitor};

const FIXTURE: &[u8] = include_bytes!("fixtures/nav_probe.binbook");

#[derive(Default)]
struct Collector {
    issues: Vec<ValidationIssue>,
}

impl ValidationVisitor for Collector {
    fn visit(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }
}

fn section_entry_offset(data: &[u8], section_id: u16) -> usize {
    let table_offset =
        usize::try_from(u64::from_le_bytes(data[24..32].try_into().unwrap())).unwrap();
    let count = usize::from(u16::from_le_bytes(data[38..40].try_into().unwrap()));
    (0..count)
        .map(|index| table_offset + index * 40)
        .find(|offset| {
            u16::from_le_bytes(data[*offset..*offset + 2].try_into().unwrap()) == section_id
        })
        .unwrap()
}

fn section_data_offset(data: &[u8], section_id: u16) -> usize {
    let entry = section_entry_offset(data, section_id);
    usize::try_from(u64::from_le_bytes(
        data[entry + 4..entry + 12].try_into().unwrap(),
    ))
    .unwrap()
}

fn codes(data: &[u8]) -> Vec<ValidationCode> {
    let mut collector = Collector::default();
    let mut section_scratch = [0_u8; 1024];
    let mut record_scratch = [0_u8; 256];
    validate_all(
        SliceSource::new(data),
        &mut section_scratch,
        &mut record_scratch,
        &mut collector,
    )
    .expect("slice reads are infallible for validator-checked bounds");
    collector
        .issues
        .into_iter()
        .map(|issue| issue.code)
        .collect()
}

fn assert_has_code(data: &[u8], expected: ValidationCode) {
    let actual = codes(data);
    assert!(
        actual.contains(&expected),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn valid_fixture_has_no_validation_issues() {
    assert_eq!(codes(FIXTURE), []);
}

#[test]
fn reports_distinct_file_section_and_policy_failures() {
    let mut bounds = FIXTURE.to_vec();
    let beyond_end = u64::try_from(bounds.len()).unwrap() + 1;
    bounds[16..24].copy_from_slice(&beyond_end.to_le_bytes());
    assert_has_code(&bounds, ValidationCode::Bounds);

    let mut order = FIXTURE.to_vec();
    let first = section_entry_offset(&order, 1);
    let second = section_entry_offset(&order, 10);
    order[first..first + 2].copy_from_slice(&10_u16.to_le_bytes());
    order[second..second + 2].copy_from_slice(&1_u16.to_le_bytes());
    assert_has_code(&order, ValidationCode::Ordering);

    let mut reserved = FIXTURE.to_vec();
    reserved[8] = 1;
    assert_has_code(&reserved, ValidationCode::ReservedBytes);

    let mut crc = FIXTURE.to_vec();
    let display = section_entry_offset(&crc, 10);
    crc[display + 28..display + 32].copy_from_slice(&1_u32.to_le_bytes());
    assert_has_code(&crc, ValidationCode::SectionCrc);

    let mut features = FIXTURE.to_vec();
    let requirements = section_data_offset(&features, 12);
    features[requirements + 8..requirements + 16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_has_code(&features, ValidationCode::RequiredFeatures);
}

#[test]
fn reports_distinct_profile_string_page_and_page_crc_failures() {
    let mut profile = FIXTURE.to_vec();
    let display = section_data_offset(&profile, 10);
    profile[display + 24..display + 26].copy_from_slice(&0_u16.to_le_bytes());
    assert_has_code(&profile, ValidationCode::Profile);

    let mut string = FIXTURE.to_vec();
    let display = section_data_offset(&string, 10);
    string[display..display + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_has_code(&string, ValidationCode::StringReference);

    let mut plane = FIXTURE.to_vec();
    let page = section_data_offset(&plane, 40);
    plane[page + 44] = 0x80;
    assert_has_code(&plane, ValidationCode::Plane);

    let mut page_crc = FIXTURE.to_vec();
    let page = section_data_offset(&page_crc, 40);
    page_crc[page + 16..page + 20].copy_from_slice(&1_u32.to_le_bytes());
    assert_has_code(&page_crc, ValidationCode::PageCrc);
}

#[test]
fn reports_distinct_cross_record_and_font_failures() {
    let mut chunk = FIXTURE.to_vec();
    let offset = section_data_offset(&chunk, 44);
    chunk[offset + 10] = 1;
    assert_has_code(&chunk, ValidationCode::Chunk);

    let mut transition = FIXTURE.to_vec();
    let offset = section_data_offset(&transition, 45);
    transition[offset + 16] = 1;
    assert_has_code(&transition, ValidationCode::Transition);

    let mut nav = FIXTURE.to_vec();
    let nav_entry = section_entry_offset(&nav, 41);
    let offset = section_data_offset(&nav, 44);
    nav[nav_entry + 4..nav_entry + 12].copy_from_slice(&(offset as u64).to_le_bytes());
    nav[nav_entry + 12..nav_entry + 20].copy_from_slice(&48_u64.to_le_bytes());
    nav[nav_entry + 24..nav_entry + 28].copy_from_slice(&1_u32.to_le_bytes());
    nav[offset..offset + 48].fill(0);
    nav[offset + 32..offset + 36].copy_from_slice(&1_u32.to_le_bytes());
    assert_has_code(&nav, ValidationCode::Navigation);

    let mut chapter = FIXTURE.to_vec();
    let chunk_offset = section_data_offset(&chapter, 44);
    let nav_entry = section_entry_offset(&chapter, 41);
    chapter[nav_entry + 4..nav_entry + 12].copy_from_slice(&(chunk_offset as u64).to_le_bytes());
    chapter[nav_entry + 12..nav_entry + 20].copy_from_slice(&48_u64.to_le_bytes());
    chapter[nav_entry + 24..nav_entry + 28].copy_from_slice(&1_u32.to_le_bytes());
    chapter[chunk_offset..chunk_offset + 48].fill(0);
    let offset = chunk_offset + 48;
    let chapter_entry = section_entry_offset(&chapter, 43);
    chapter[chapter_entry + 4..chapter_entry + 12].copy_from_slice(&(offset as u64).to_le_bytes());
    chapter[chapter_entry + 12..chapter_entry + 20].copy_from_slice(&32_u64.to_le_bytes());
    chapter[chapter_entry + 24..chapter_entry + 28].copy_from_slice(&1_u32.to_le_bytes());
    chapter[offset..offset + 32].fill(0);
    chapter[offset + 22..offset + 24].copy_from_slice(&3_u16.to_le_bytes());
    assert_has_code(&chapter, ValidationCode::Chapter);

    let mut font = FIXTURE.to_vec();
    let font_entry = section_entry_offset(&font, 35);
    let display_offset = u64::try_from(section_data_offset(&font, 10)).unwrap();
    font[font_entry + 4..font_entry + 12].copy_from_slice(&display_offset.to_le_bytes());
    font[font_entry + 12..font_entry + 20].copy_from_slice(&80_u64.to_le_bytes());
    font[font_entry + 24..font_entry + 28].copy_from_slice(&1_u32.to_le_bytes());
    assert_has_code(&font, ValidationCode::FontResource);
}
