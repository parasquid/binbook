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
pub enum PageTurn {
    Previous,
    Next,
    First,
    Last,
}

pub fn page_turn_for_button(button: Button) -> Option<PageTurn> {
    match button {
        Button::Right | Button::Down => Some(PageTurn::Next),
        Button::Left | Button::Up => Some(PageTurn::Previous),
        Button::Back | Button::Select | Button::Power => None,
    }
}

pub fn target_page_for_button(button: Button) -> PageTurn {
    match button {
        Button::Right | Button::Down => PageTurn::Next,
        Button::Left | Button::Up => PageTurn::Previous,
        Button::Back => PageTurn::First,
        Button::Select => PageTurn::Last,
        Button::Power => PageTurn::Next,
    }
}

pub fn apply_page_turn(current_page: u32, page_count: u32, turn: PageTurn) -> u32 {
    if page_count == 0 {
        return 0;
    }
    match turn {
        PageTurn::Next => current_page.saturating_add(1).min(page_count - 1),
        PageTurn::Previous => current_page.saturating_sub(1),
        PageTurn::First => 0,
        PageTurn::Last => page_count - 1,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Press(Button),
    Release(Button),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDecision {
    Unchanged,
    Press(Button),
    Released,
    SuppressedByCooldown {
        observed: Option<Button>,
        elapsed_ms: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPollOutcome {
    pub previous: Option<Button>,
    pub observed: Option<Button>,
    pub elapsed_since_last_press_ms: u32,
    pub decision: InputDecision,
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

    pub fn poll_raw(&mut self, ch1: u16, ch2: u16, now_ms: u64) -> Option<ButtonEvent> {
        match self.poll_raw_detailed(ch1, ch2, now_ms).decision {
            InputDecision::Press(button) => Some(ButtonEvent::Press(button)),
            InputDecision::Unchanged
            | InputDecision::Released
            | InputDecision::SuppressedByCooldown { .. } => None,
        }
    }

    pub fn poll_raw_detailed(&mut self, ch1: u16, ch2: u16, now_ms: u64) -> InputPollOutcome {
        let previous = self.last_button;
        let observed = decode_buttons(ch1, ch2);
        let elapsed = now_ms.saturating_sub(self.last_press_time);
        let elapsed_ms = elapsed.min(u32::MAX as u64) as u32;
        let decision = if observed == previous {
            InputDecision::Unchanged
        } else if elapsed > self.cooldown_ms as u64 {
            self.last_press_time = now_ms;
            match observed {
                Some(button) => InputDecision::Press(button),
                None => InputDecision::Released,
            }
        } else {
            InputDecision::SuppressedByCooldown {
                observed,
                elapsed_ms,
            }
        };
        self.last_button = observed;
        InputPollOutcome {
            previous,
            observed,
            elapsed_since_last_press_ms: elapsed_ms,
            decision,
        }
    }

    pub fn poll(
        &mut self,
        ch1: &impl AdcPin,
        ch2: &impl AdcPin,
        now_ms: u64,
    ) -> Option<ButtonEvent> {
        self.poll_raw(ch1.read().unwrap_or(0), ch2.read().unwrap_or(0), now_ms)
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn decode_buttons(ch1: u16, ch2: u16) -> Option<Button> {
    let ch1_button = if ch1 <= 750 {
        Some(Button::Right)
    } else if ch1 <= 1750 {
        Some(Button::Left)
    } else if ch1 <= 3000 {
        Some(Button::Select)
    } else if ch1 <= 3800 {
        Some(Button::Back)
    } else {
        None
    };

    let ch2_button = if ch2 <= 750 {
        Some(Button::Down)
    } else if ch2 <= 2400 {
        Some(Button::Up)
    } else {
        None
    };

    ch1_button.or(ch2_button)
}
