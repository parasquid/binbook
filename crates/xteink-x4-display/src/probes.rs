use ssd1677_driver::{BusyWaitObserver, NoopBusyWaitObserver};

pub const DISPLAY_WIDTH: u16 = 800;
pub const DISPLAY_HEIGHT: u16 = 480;
pub const DISPLAY_ROW_BYTES: usize = 100;
pub const PROBE_BOX_WIDTH: u16 = 128;
pub const PROBE_BOX_HEIGHT: u16 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    FullRefreshCurrent,
    ClearWhite,
    WindowCorners,
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
