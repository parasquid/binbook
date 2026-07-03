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
    raw_button: Option<Button>,
    reported_button: Option<Button>,
    last_decision_time: u64,
    ch1: LadderState,
    ch2: LadderState,
}

impl InputState {
    pub const DEBOUNCE_MS: u32 = 30;

    pub const fn new() -> Self {
        Self {
            raw_button: None,
            reported_button: None,
            last_decision_time: 0,
            ch1: LadderState::new(),
            ch2: LadderState::new(),
        }
    }

    pub fn last_button(&self) -> Option<Button> {
        self.reported_button
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
        let previous = self.raw_button;
        let raw_ch1 = decode_ch1_button(ch1);
        let raw_ch2 = decode_ch2_button(ch2);
        let raw_observed = raw_ch1.or(raw_ch2);
        self.raw_button = raw_observed;

        self.ch1.update(raw_ch1, now_ms);
        self.ch2.update(raw_ch2, now_ms);

        let debounced_observed = self.ch1.stable.or(self.ch2.stable);
        let elapsed = now_ms.saturating_sub(self.last_decision_time);
        let elapsed_ms = elapsed.min(u32::MAX as u64) as u32;
        let decision = if debounced_observed == self.reported_button {
            InputDecision::Unchanged
        } else {
            self.last_decision_time = now_ms;
            self.reported_button = debounced_observed;
            match debounced_observed {
                Some(button) => InputDecision::Press(button),
                None => InputDecision::Released,
            }
        };
        InputPollOutcome {
            previous,
            observed: raw_observed,
            elapsed_since_last_press_ms: elapsed_ms,
            decision,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LadderState {
    candidate: Option<Button>,
    candidate_since_ms: u64,
    stable: Option<Button>,
}

impl LadderState {
    const fn new() -> Self {
        Self {
            candidate: None,
            candidate_since_ms: 0,
            stable: None,
        }
    }

    fn update(&mut self, observed: Option<Button>, now_ms: u64) {
        if observed != self.candidate {
            self.candidate = observed;
            self.candidate_since_ms = now_ms;
            return;
        }

        let elapsed = now_ms.saturating_sub(self.candidate_since_ms);
        if elapsed >= InputState::DEBOUNCE_MS as u64 {
            self.stable = observed;
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn decode_buttons(ch1: u16, ch2: u16) -> Option<Button> {
    decode_ch1_button(ch1).or(decode_ch2_button(ch2))
}

fn decode_ch1_button(ch1: u16) -> Option<Button> {
    if ch1 <= 750 {
        Some(Button::Right)
    } else if ch1 <= 1750 {
        Some(Button::Left)
    } else if ch1 <= 3000 {
        Some(Button::Select)
    } else if ch1 <= 3800 {
        Some(Button::Back)
    } else {
        None
    }
}

fn decode_ch2_button(ch2: u16) -> Option<Button> {
    if ch2 <= 750 {
        Some(Button::Down)
    } else if ch2 <= 2400 {
        Some(Button::Up)
    } else {
        None
    }
}
