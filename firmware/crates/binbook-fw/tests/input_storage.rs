use binbook_fw::flash::{FlashStorage, FILE_ENTRY_SIZE};
use binbook_fw::input::{
    apply_page_turn, decode_buttons, page_turn_for_button, Button, ButtonEvent, InputDecision,
    InputPollOutcome, InputState, PageTurn,
};
use embedded_storage::nor_flash::NorFlash as _;

#[test]
fn decodes_adc_ladder_buttons() {
    assert_eq!(decode_buttons(500, 4095), Some(Button::Right));
    assert_eq!(decode_buttons(1000, 4095), Some(Button::Left));
    assert_eq!(decode_buttons(1800, 4095), Some(Button::Select));
    assert_eq!(decode_buttons(2800, 4095), Some(Button::Select));
    assert_eq!(decode_buttons(3500, 4095), Some(Button::Back));
    assert_eq!(decode_buttons(4095, 500), Some(Button::Down));
    assert_eq!(decode_buttons(4095, 1500), Some(Button::Up));
    assert_eq!(decode_buttons(4095, 2280), Some(Button::Up));
    assert_eq!(decode_buttons(4095, 4095), None);
}

#[test]
fn finds_valid_flash_file_table_entry_by_name() {
    let mut flash = MockFlash::new();
    flash.write_entry(0, "sample.binbook", 512, 4096);

    let mut storage = FlashStorage::new(flash);
    let info = storage.find("sample.binbook").unwrap().unwrap();

    assert_eq!(info.offset, 512);
    assert_eq!(info.size, 4096);
}

#[test]
fn reads_file_bytes_relative_to_flash_file_offset() {
    let mut flash = MockFlash::new();
    flash.write_entry(0, "sample.binbook", 64, 4);
    flash.write(64, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

    let mut storage = FlashStorage::new(flash);
    let info = storage.find("sample.binbook").unwrap().unwrap();

    let mut out = [0u8; 2];
    storage.read_file(&info, 1, &mut out).unwrap();

    assert_eq!(out, [0xAD, 0xBE]);
}

#[test]
fn page_source_reads_exact_compressed_page_slice() {
    use binbook_fw::book::{PageExtent, PageSource};

    struct Bytes<'a>(&'a [u8]);

    impl PageSource for Bytes<'_> {
        type Error = ();

        fn read_at(&self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
            let offset = offset as usize;
            out.copy_from_slice(&self.0[offset..offset + out.len()]);
            Ok(())
        }
    }

    let source = Bytes(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let extent = PageExtent { offset: 2, size: 3 };
    let mut out = [0u8; 3];

    source.read_page(&extent, &mut out).unwrap();

    assert_eq!(out, [2, 3, 4]);
}

#[test]
fn directional_buttons_map_to_page_turns() {
    assert_eq!(page_turn_for_button(Button::Right), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Down), Some(PageTurn::Next));
    assert_eq!(page_turn_for_button(Button::Left), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Up), Some(PageTurn::Previous));
    assert_eq!(page_turn_for_button(Button::Select), None);
    assert_eq!(page_turn_for_button(Button::Back), None);
    assert_eq!(page_turn_for_button(Button::Power), None);
}

#[test]
fn page_turns_clamp_at_book_edges() {
    assert_eq!(apply_page_turn(0, 4, PageTurn::Previous), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::Next), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Previous), 1);
    assert_eq!(apply_page_turn(2, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(3, 4, PageTurn::Next), 3);
    assert_eq!(apply_page_turn(0, 0, PageTurn::Next), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::First), 0);
    assert_eq!(apply_page_turn(3, 4, PageTurn::First), 0);
    assert_eq!(apply_page_turn(0, 4, PageTurn::Last), 3);
    assert_eq!(apply_page_turn(3, 4, PageTurn::Last), 3);
}

struct MockFlash {
    bytes: [u8; 512],
}

impl MockFlash {
    fn new() -> Self {
        Self { bytes: [0xff; 512] }
    }

    fn write_entry(&mut self, index: usize, name: &str, offset: u32, size: u32) {
        let start = index * FILE_ENTRY_SIZE;
        self.bytes[start..start + 32].fill(0);
        self.bytes[start..start + name.len()].copy_from_slice(name.as_bytes());
        self.bytes[start + 32..start + 36].copy_from_slice(&offset.to_le_bytes());
        self.bytes[start + 36..start + 40].copy_from_slice(&size.to_le_bytes());
        self.bytes[start + 40] = 0;
    }
}

impl embedded_storage::nor_flash::ErrorType for MockFlash {
    type Error = core::convert::Infallible;
}

impl embedded_storage::nor_flash::ReadNorFlash for MockFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, output: &mut [u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        output.copy_from_slice(&self.bytes[start..start + output.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }
}

impl embedded_storage::nor_flash::NorFlash for MockFlash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 1;

    fn write(&mut self, offset: u32, input: &[u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        self.bytes[start..start + input.len()].copy_from_slice(input);
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.bytes[from as usize..to as usize].fill(0xff);
        Ok(())
    }
}

#[test]
fn raw_poll_emits_one_press_per_button_transition() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(4095, 4095, 0), None);
    assert_eq!(
        input.poll_raw(500, 4095, 150),
        Some(ButtonEvent::Press(Button::Right))
    );
    assert_eq!(input.poll_raw(500, 4095, 300), None);
    assert_eq!(input.poll_raw(4095, 4095, 450), None);
    assert_eq!(
        input.poll_raw(4095, 500, 600),
        Some(ButtonEvent::Press(Button::Down))
    );
}

#[test]
fn raw_poll_suppresses_transitions_inside_cooldown() {
    let mut input = InputState::new();

    assert_eq!(input.poll_raw(500, 4095, 50), None);
    assert_eq!(input.poll_raw(500, 4095, 150), None);
}

#[test]
fn detailed_poll_reports_the_initial_cooldown_suppression() {
    let mut input = InputState::new();

    assert_eq!(
        input.poll_raw_detailed(500, 4095, 50),
        InputPollOutcome {
            previous: None,
            observed: Some(Button::Right),
            elapsed_since_last_press_ms: 50,
            decision: InputDecision::SuppressedByCooldown {
                observed: Some(Button::Right),
                elapsed_ms: 50,
            },
        }
    );
}

#[test]
fn detailed_poll_reports_one_accepted_press() {
    let mut input = InputState::new();

    assert_eq!(
        input.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );
    assert_eq!(
        input.poll_raw_detailed(500, 4095, 250).decision,
        InputDecision::Unchanged
    );
}

#[test]
fn detailed_poll_preserves_exact_cooldown_and_suppressed_state_change() {
    let mut input = InputState::new();
    assert_eq!(
        input.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );

    assert_eq!(
        input.poll_raw_detailed(1000, 4095, 201).decision,
        InputDecision::SuppressedByCooldown {
            observed: Some(Button::Left),
            elapsed_ms: 100,
        }
    );
    assert_eq!(input.last_button(), Some(Button::Left));
    assert_eq!(
        input.poll_raw_detailed(1000, 4095, 302).decision,
        InputDecision::Unchanged
    );
}

#[test]
fn detailed_poll_reports_release_without_changing_legacy_events() {
    let mut detailed = InputState::new();
    assert_eq!(
        detailed.poll_raw_detailed(500, 4095, 101).decision,
        InputDecision::Press(Button::Right)
    );
    assert_eq!(
        detailed.poll_raw_detailed(4095, 4095, 202).decision,
        InputDecision::Released
    );

    let mut legacy = InputState::new();
    assert_eq!(
        legacy.poll_raw(500, 4095, 101),
        Some(ButtonEvent::Press(Button::Right))
    );
    assert_eq!(legacy.poll_raw(4095, 4095, 202), None);
}
