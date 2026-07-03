use ssd1677_driver::{BusyWaitObserver, NoopBusyWaitObserver};

pub use crate::panel::{custom_lut_candidate_waveform, CustomLutCandidate};
pub use crate::profile::{
    PHYSICAL_HEIGHT as DISPLAY_HEIGHT, PHYSICAL_WIDTH as DISPLAY_WIDTH,
    ROW_BYTES as DISPLAY_ROW_BYTES,
};

pub const PROBE_BOX_WIDTH: u16 = 128;
pub const PROBE_BOX_HEIGHT: u16 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
    PartialWindowBusyMatrix,
    CustomLutFastA,
    CustomLutFastB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialWindowPolarity {
    BlackOnWhite,
    WhiteOnBlack,
}

impl PartialWindowPolarity {
    pub const fn code(self) -> i32 {
        match self {
            Self::WhiteOnBlack => 0,
            Self::BlackOnWhite => 1,
        }
    }

    const fn previous_value(self) -> u8 {
        match self {
            Self::BlackOnWhite => 0xff,
            Self::WhiteOnBlack => 0x00,
        }
    }

    const fn target_value(self) -> u8 {
        match self {
            Self::BlackOnWhite => 0x00,
            Self::WhiteOnBlack => 0xff,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialWindowBusyCase {
    pub name: &'static str,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl PartialWindowBusyCase {
    pub fn bytes_per_plane(self) -> u32 {
        u32::from(self.width.div_ceil(8)) * u32::from(self.height)
    }
}

#[must_use]
pub const fn partial_window_busy_cases() -> [PartialWindowBusyCase; 6] {
    [
        PartialWindowBusyCase {
            name: "full",
            x: 0,
            y: 0,
            width: DISPLAY_WIDTH,
            height: DISPLAY_HEIGHT,
        },
        PartialWindowBusyCase {
            name: "half-height",
            x: 0,
            y: 120,
            width: DISPLAY_WIDTH,
            height: 240,
        },
        PartialWindowBusyCase {
            name: "stripe-64",
            x: 0,
            y: 208,
            width: DISPLAY_WIDTH,
            height: 64,
        },
        PartialWindowBusyCase {
            name: "strip-16",
            x: 0,
            y: 232,
            width: DISPLAY_WIDTH,
            height: 16,
        },
        PartialWindowBusyCase {
            name: "center-box",
            x: 300,
            y: 190,
            width: 200,
            height: 100,
        },
        PartialWindowBusyCase {
            name: "page-number-box",
            x: 280,
            y: 0,
            width: 240,
            height: 80,
        },
    ]
}

#[must_use]
pub const fn probe_polarity(round: u16) -> PartialWindowPolarity {
    if round % 2 == 0 {
        PartialWindowPolarity::WhiteOnBlack
    } else {
        PartialWindowPolarity::BlackOnWhite
    }
}

#[must_use]
pub const fn corner_windows() -> [(u16, u16, u16, u16); 4] {
    [
        (0, 0, PROBE_BOX_WIDTH, PROBE_BOX_HEIGHT),
        (
            DISPLAY_WIDTH - PROBE_BOX_WIDTH,
            0,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
        (
            0,
            DISPLAY_HEIGHT - PROBE_BOX_HEIGHT,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
        (
            DISPLAY_WIDTH - PROBE_BOX_WIDTH,
            DISPLAY_HEIGHT - PROBE_BOX_HEIGHT,
            PROBE_BOX_WIDTH,
            PROBE_BOX_HEIGHT,
        ),
    ]
}

pub fn build_corner_row(row: u16, output: &mut [u8; DISPLAY_ROW_BYTES]) {
    output.fill(0xff);
    if !(PROBE_BOX_HEIGHT..DISPLAY_HEIGHT - PROBE_BOX_HEIGHT).contains(&row) {
        let width = usize::from(PROBE_BOX_WIDTH / 8);
        output[..width].fill(0);
        output[DISPLAY_ROW_BYTES - width..].fill(0);
    }
}

pub trait PartialWindowBusyObserver {
    fn now_ms(&self) -> u64 {
        0
    }
    fn case_start(&mut self, _case: u8, _round: u16, _polarity: PartialWindowPolarity) {}
    fn geometry(&mut self, _case: u8, _window: PartialWindowBusyCase) {}
    fn size(&mut self, _case: u8, _bytes_per_plane: u32, _total_bytes: u32) {}
    fn ram_write(&mut self, _case: u8, _red_ms: u32, _black_ms: u32) {}
    fn busy(&mut self, _case: u8, _busy_ms: u32, _outcome: ssd1677_driver::BusyWaitOutcome) {}
    fn case_end(&mut self, _case: u8, _duration_ms: u32, _status: i32) {}
}

impl PartialWindowBusyObserver for () {}

pub async fn partial_window_busy_matrix<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
    rounds: u16,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    partial_window_busy_matrix_observed(panel, delay, rounds, &mut ()).await
}

pub async fn partial_window_busy_matrix_observed<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
    rounds: u16,
    observer: &mut impl PartialWindowBusyObserver,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    let rounds = rounds.max(1);
    for round in 1..=rounds {
        let polarity = probe_polarity(round);
        for (index, case) in partial_window_busy_cases().into_iter().enumerate() {
            let case_index = index as u8;
            let case_start = observer.now_ms();
            observer.case_start(case_index, round, polarity);
            observer.geometry(case_index, case);
            let bytes_per_plane = case.bytes_per_plane();
            observer.size(
                case_index,
                bytes_per_plane,
                bytes_per_plane.saturating_mul(2),
            );

            let red_start = observer.now_ms();
            panel.controller().write_red_solid_window(
                case.x,
                case.y,
                case.width,
                case.height,
                polarity.previous_value(),
            )?;
            let red_ms = elapsed_u32(red_start, observer.now_ms());

            let black_start = observer.now_ms();
            panel.controller().write_solid_window(
                case.x,
                case.y,
                case.width,
                case.height,
                polarity.target_value(),
            )?;
            let black_ms = elapsed_u32(black_start, observer.now_ms());
            observer.ram_write(case_index, red_ms, black_ms);

            panel
                .controller()
                .trigger_refresh(crate::panel::RefreshMode::Partial)?;
            let mut busy_observer = PartialWindowBusyWaitObserver {
                case: case_index,
                observer,
            };
            panel.wait_ready_observed(delay, &mut busy_observer).await?;
            observer.case_end(case_index, elapsed_u32(case_start, observer.now_ms()), 0);
        }
    }
    Ok(())
}

struct PartialWindowBusyWaitObserver<'a, O> {
    case: u8,
    observer: &'a mut O,
}

impl<O: PartialWindowBusyObserver> BusyWaitObserver for PartialWindowBusyWaitObserver<'_, O> {
    fn busy_wait_start(&mut self, _timeout_ms: u32, _busy_state: Option<bool>) {}

    fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: ssd1677_driver::BusyWaitOutcome) {
        self.observer.busy(self.case, elapsed_ms, outcome);
    }
}

fn elapsed_u32(start: u64, end: u64) -> u32 {
    end.saturating_sub(start).min(u32::MAX as u64) as u32
}

pub async fn clear_white<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    clear_white_observed(panel, delay, &mut NoopBusyWaitObserver).await
}

pub async fn clear_white_observed<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xff))?;
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xff))?;
    panel
        .refresh_observed(crate::panel::RefreshMode::Full, delay, observer)
        .await?;
    Ok(())
}

pub async fn window_corners<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    window_corners_observed(panel, delay, &mut NoopBusyWaitObserver).await
}

pub async fn window_corners_observed<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xff))?;
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xff))?;
    for (x, y, width, height) in corner_windows() {
        panel
            .controller()
            .write_solid_window(x, y, width, height, 0)?;
    }
    for (x, y, width, height) in corner_windows() {
        panel
            .controller()
            .write_red_solid_window(x, y, width, height, 0)?;
    }
    panel
        .refresh_observed(crate::panel::RefreshMode::Full, delay, observer)
        .await?;
    Ok(())
}

pub async fn custom_lut_probe_observed<SPI, DC, RST, BUSY, D>(
    panel: &mut crate::panel::X4Panel<SPI, DC, RST, BUSY>,
    candidate: CustomLutCandidate,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> crate::DisplayResult<()>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    D: embedded_hal_async::delay::DelayNs,
{
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, custom_lut_previous_row)?;
    panel
        .controller()
        .set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    panel
        .controller()
        .write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, custom_lut_target_row)?;
    panel.load_custom_lut_candidate(candidate)?;
    panel
        .controller()
        .trigger_refresh(crate::panel::RefreshMode::StagedGrayscale)?;
    panel.wait_ready_observed(delay, observer).await?;
    Ok(())
}

fn custom_lut_previous_row(row: u16, output: &mut [u8; DISPLAY_ROW_BYTES]) {
    output.fill(0xff);
    if row < 12 || row >= DISPLAY_HEIGHT - 12 {
        output.fill(0x00);
    }
}

fn custom_lut_target_row(row: u16, output: &mut [u8; DISPLAY_ROW_BYTES]) {
    output.fill(if (row / 24) % 2 == 0 { 0x00 } else { 0xff });
    output[0] = 0x00;
    output[DISPLAY_ROW_BYTES - 1] = 0x00;
}
