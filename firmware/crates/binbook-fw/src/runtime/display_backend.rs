use portable_atomic::Ordering;

use xteink_x4_display::{
    engine::DisplayBackend,
    events::OperationOutcome,
    panel::X4Panel,
    render::{self, OverlayControl},
};

use binbook_fw::board::{BoardSpiDevice, DisplayDelay};
use binbook_fw::runtime_engine::{RuntimeEvent, RuntimeEventKind};

use super::{REQUEST_EPOCH, RUNTIME_EVENT_CHANNEL};

pub(super) struct HardwareDisplayBackend<'a, SPI, CS, DC, RST, BUSY> {
    pub(super) display: X4Panel<BoardSpiDevice<SPI, CS>, DC, RST, BUSY>,
    pub(super) book: binbook_core::Book<binbook_core::SliceSource<'a>>,
    pub(super) delay: DisplayDelay,
    pub(super) compressed: [u8; 768],
    pub(super) decoded: [u8; 300],
    pub(super) black: [u8; 100],
    pub(super) red: [u8; 100],
}

impl<'a, SPI, CS, DC, RST, BUSY> DisplayBackend
    for HardwareDisplayBackend<'a, SPI, CS, DC, RST, BUSY>
where
    SPI: embedded_hal::spi::SpiBus<u8>,
    CS: embedded_hal::digital::OutputPin,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
{
    fn timestamp_ms(&self) -> Option<u64> {
        Some(embassy_time::Instant::now().as_millis())
    }

    fn request_epoch(&self) -> u32 {
        REQUEST_EPOCH.load(Ordering::Acquire)
    }

    async fn init_grayscale(&mut self) -> xteink_x4_display::DisplayResult<()> {
        self.display.init_absolute_gray_async(&mut self.delay).await
    }

    async fn render_grayscale(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> xteink_x4_display::DisplayResult<OperationOutcome> {
        let mut buffers = xteink_x4_display::buffers::RenderBuffers::new(
            &mut self.compressed,
            &mut self.decoded,
            &mut self.black,
            &mut self.red,
        );
        render::render_staged_overlay(
            &mut self.display,
            &mut self.book,
            page,
            &mut buffers,
            OverlayControl {
                expected_epoch,
                epoch: || REQUEST_EPOCH.load(Ordering::Acquire),
                on_activate: || {
                    let timestamp_ms = embassy_time::Instant::now().as_millis();
                    let sender = RUNTIME_EVENT_CHANNEL.sender();
                    let _ = sender.try_send(RuntimeEvent {
                        timestamp_ms,
                        kind: RuntimeEventKind::WaveformSelected {
                            waveform_hint: binbook_core::WAVEFORM_SSD1677_STAGED_GRAY2,
                            lut_revision: xteink_x4_display::panel::STAGED_GRAY_LUT_REVISION,
                        },
                    });
                    let _ = sender.try_send(RuntimeEvent {
                        timestamp_ms,
                        kind: RuntimeEventKind::GrayActivated { page },
                    });
                },
            },
            &mut self.delay,
        )
        .await
    }

    async fn init_bw(&mut self) -> xteink_x4_display::DisplayResult<()> {
        self.display.init_bw_async(&mut self.delay).await
    }

    async fn render_bw(&mut self, from: u32, target: u32) -> xteink_x4_display::DisplayResult<()> {
        let mut buffers = xteink_x4_display::buffers::RenderBuffers::new(
            &mut self.compressed,
            &mut self.decoded,
            &mut self.black,
            &mut self.red,
        );
        render::render_bw_differential(
            &mut self.display,
            &mut self.book,
            from,
            target,
            &mut buffers,
            &mut self.delay,
        )
        .await
    }

    async fn sync_bw_base(
        &mut self,
        page: u32,
        expected_epoch: u32,
    ) -> xteink_x4_display::DisplayResult<OperationOutcome> {
        let mut buffers = xteink_x4_display::buffers::RenderBuffers::new(
            &mut self.compressed,
            &mut self.decoded,
            &mut self.black,
            &mut self.red,
        );
        render::sync_bw_base(
            &mut self.display,
            &mut self.book,
            page,
            &mut buffers,
            expected_epoch,
            || REQUEST_EPOCH.load(Ordering::Acquire),
            &mut self.delay,
        )
        .await
    }

    async fn recover_bw(&mut self, page: u32) -> xteink_x4_display::DisplayResult<()> {
        let mut buffers = xteink_x4_display::buffers::RenderBuffers::new(
            &mut self.compressed,
            &mut self.decoded,
            &mut self.black,
            &mut self.red,
        );
        render::recovery_seed(
            &mut self.display,
            &mut self.book,
            page,
            &mut buffers,
            &mut self.delay,
        )
        .await
    }

    async fn run_probe(
        &mut self,
        kind: xteink_x4_display::probes::ProbeKind,
        page: u32,
    ) -> xteink_x4_display::DisplayResult<()> {
        #[cfg(feature = "diagnostic-console")]
        match kind {
            xteink_x4_display::probes::ProbeKind::FullRefreshCurrent => {
                let mut buffers = xteink_x4_display::buffers::RenderBuffers::new(
                    &mut self.compressed,
                    &mut self.decoded,
                    &mut self.black,
                    &mut self.red,
                );
                render::render_absolute_gray(
                    &mut self.display,
                    &mut self.book,
                    page,
                    &mut buffers,
                    &mut self.delay,
                )
                .await
            }
            xteink_x4_display::probes::ProbeKind::ClearWhite => {
                xteink_x4_display::probes::clear_white(&mut self.display, &mut self.delay).await
            }
            xteink_x4_display::probes::ProbeKind::WindowCorners => {
                xteink_x4_display::probes::window_corners(&mut self.display, &mut self.delay).await
            }
        }
        #[cfg(not(feature = "diagnostic-console"))]
        {
            let _ = (kind, page);
            Err(xteink_x4_display::DisplayError::InvalidState)
        }
    }
}
