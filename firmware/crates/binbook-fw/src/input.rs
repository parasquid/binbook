use xteink_hal::AdcPin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Up,
    Down,
    Back,
    Select,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Press(Button),
    Release(Button),
}

#[derive(Debug, Clone, Copy)]
pub struct InputState {
    last_button: Option<Button>,
    last_press_time: u64,
    cooldown_ms: u32,
}

impl InputState {
    pub const fn new() -> Self {
        Self {
            last_button: None,
            last_press_time: 0,
            cooldown_ms: 100,
        }
    }

    pub fn last_button(&self) -> Option<Button> {
        self.last_button
    }

    pub fn poll(
        &mut self,
        ch1: &impl AdcPin,
        ch2: &impl AdcPin,
        now_ms: u64,
    ) -> Option<ButtonEvent> {
        let button = decode_buttons(ch1.read().unwrap_or(0), ch2.read().unwrap_or(0));

        let event = if button != self.last_button {
            if now_ms.saturating_sub(self.last_press_time) > self.cooldown_ms as u64 {
                self.last_press_time = now_ms;
                button.map(ButtonEvent::Press)
            } else {
                None
            }
        } else {
            None
        };

        self.last_button = button;
        event
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn decode_buttons(ch1: u16, ch2: u16) -> Option<Button> {
    if ch1 > 2200 {
        Some(Button::Back)
    } else if ch1 > 1600 {
        Some(Button::Select)
    } else if ch1 > 750 {
        Some(Button::Left)
    } else if ch1 > 10 {
        Some(Button::Right)
    } else if ch2 > 750 {
        Some(Button::Up)
    } else if ch2 > 10 {
        Some(Button::Down)
    } else {
        None
    }
}
