use binbook_core::{Book, Error, FormatError, ReadAt, SliceSource};

const FIXTURE: &[u8] = include_bytes!("fixtures/nav_probe.binbook");

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFailure;

struct FailingSource;

impl ReadAt for FailingSource {
    type Error = SourceFailure;

    fn len(&mut self) -> Result<u64, Self::Error> {
        Ok(512)
    }

    fn read_exact_at(&mut self, _offset: u64, _out: &mut [u8]) -> Result<(), Self::Error> {
        Err(SourceFailure)
    }
}

#[test]
fn source_failures_remain_distinct_from_format_failures() {
    let mut scratch = [0_u8; 1024];
    assert!(matches!(
        Book::open(FailingSource, &mut scratch),
        Err(Error::Source(SourceFailure))
    ));

    let mut invalid = FIXTURE.to_vec();
    invalid[..8].copy_from_slice(b"NOTBOOK\0");
    assert!(matches!(
        Book::open(SliceSource::new(&invalid), &mut scratch),
        Err(Error::Format(FormatError::InvalidMagic))
    ));
}

#[test]
fn opening_reports_exact_section_scratch_requirement() {
    let mut scratch = [0_u8; 759];
    assert!(matches!(
        Book::open(SliceSource::new(FIXTURE), &mut scratch),
        Err(Error::BufferTooSmall {
            required: 760,
            provided: 759,
        })
    ));
}

#[test]
fn opening_rejects_declared_file_bounds() {
    let mut invalid = FIXTURE.to_vec();
    let invalid_length = u64::try_from(invalid.len()).unwrap() + 1;
    invalid[16..24].copy_from_slice(&invalid_length.to_le_bytes());
    let mut scratch = [0_u8; 1024];
    assert!(matches!(
        Book::open(SliceSource::new(&invalid), &mut scratch),
        Err(Error::Format(FormatError::FileOutOfBounds))
    ));
}

#[test]
fn opening_rejects_versions_section_shapes_and_missing_sections() {
    let mut scratch = [0_u8; 1024];

    let mut version = FIXTURE.to_vec();
    version[12..14].copy_from_slice(&255_u16.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&version), &mut scratch),
        Err(Error::Format(FormatError::UnsupportedVersion))
    ));

    let mut entry_size = FIXTURE.to_vec();
    entry_size[36..38].copy_from_slice(&39_u16.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&entry_size), &mut scratch),
        Err(Error::Format(FormatError::UnsupportedVersion))
    ));

    let mut missing = FIXTURE.to_vec();
    let display_entry = section_entry_offset(&missing, 10);
    missing[display_entry..display_entry + 2].copy_from_slice(&60_u16.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&missing), &mut scratch),
        Err(Error::Format(FormatError::MissingSection(10)))
    ));

    let mut missing_font_resources = FIXTURE.to_vec();
    let font_entry = section_entry_offset(&missing_font_resources, 35);
    missing_font_resources[font_entry..font_entry + 2].copy_from_slice(&60_u16.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&missing_font_resources), &mut scratch),
        Err(Error::Format(FormatError::MissingSection(35)))
    ));
}

#[test]
fn opening_rejects_section_and_string_reference_bounds() {
    let mut scratch = [0_u8; 1024];
    let mut section_bounds = FIXTURE.to_vec();
    let profile_entry = section_entry_offset(&section_bounds, 10);
    section_bounds[profile_entry + 12..profile_entry + 20].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&section_bounds), &mut scratch),
        Err(Error::Format(FormatError::FileOutOfBounds))
    ));

    let mut string_bounds = FIXTURE.to_vec();
    let profile_entry = section_entry_offset(&string_bounds, 10);
    let profile_offset = usize::try_from(u64::from_le_bytes(
        string_bounds[profile_entry + 4..profile_entry + 12]
            .try_into()
            .unwrap(),
    ))
    .unwrap();
    string_bounds[profile_offset..profile_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Book::open(SliceSource::new(&string_bounds), &mut scratch),
        Err(Error::Format(FormatError::InvalidStringRef))
    ));
}
