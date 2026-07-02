use portable_atomic::Ordering;
use ssd1677_driver::{BusyWaitObserver, BusyWaitOutcome};

use xteink_x4_display::{
    engine::DisplayBackend,
    events::OperationOutcome,
    panel::X4Panel,
    render::{self, OverlayControl},
};

use binbook_fw::board::DisplayDelay;
use binbook_fw::runtime_engine::{BusyWaitSite, BusyWaitStatus, RuntimeEvent, RuntimeEventKind};

use super::{REQUEST_EPOCH, RUNTIME_EVENT_CHANNEL};

pub(super) struct HardwareDisplayBackend<'a, SPI: embedded_hal::spi::SpiDevice<u8>, DC, RST, BUSY> {
    pub(super) display: X4Panel<SPI, DC, RST, BUSY>,
    pub(super) book: binbook_core::Book<binbook_core::SliceSource<'a>>,
    pub(super) delay: DisplayDelay,
    pub(super) compressed: [u8; 768],
    pub(super) decoded: [u8; 300],
    pub(super) black: [u8; 100],
    pub(super) red: [u8; 100],
}

struct RuntimeBusyWaitObserver {
    site: BusyWaitSite,
}

impl RuntimeBusyWaitObserver {
    const fn new(site: BusyWaitSite) -> Self {
        Self { site }
    }
}

impl BusyWaitObserver for RuntimeBusyWaitObserver {
    fn busy_wait_start(&mut self, timeout_ms: u32, busy_state: Option<bool>) {
        let _ = RUNTIME_EVENT_CHANNEL.sender().try_send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::BusyWaitStart {
                site: self.site,
                timeout_ms,
                busy_state,
            },
        });
    }

    fn busy_wait_end(&mut self, elapsed_ms: u32, outcome: BusyWaitOutcome) {
        let status = match outcome {
            BusyWaitOutcome::Ready => BusyWaitStatus::Ready,
            BusyWaitOutcome::Timeout => BusyWaitStatus::Timeout,
            BusyWaitOutcome::PinError => BusyWaitStatus::PinError,
        };
        let _ = RUNTIME_EVENT_CHANNEL.sender().try_send(RuntimeEvent {
            timestamp_ms: embassy_time::Instant::now().as_millis(),
            kind: RuntimeEventKind::BusyWaitEnd {
                site: self.site,
                elapsed_ms,
                status,
            },
        });
    }
}

impl<'a, SPI, DC, RST, BUSY> DisplayBackend for HardwareDisplayBackend<'a, SPI, DC, RST, BUSY>
where
    SPI: embedded_hal::spi::SpiDevice<u8>,
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
        let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::GrayRefresh);
        render::render_staged_overlay_observed(
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
            &mut observer,
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
        let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::BwRefresh);
        render::render_bw_differential_observed(
            &mut self.display,
            &mut self.book,
            from,
            target,
            &mut buffers,
            &mut self.delay,
            &mut observer,
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
        let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::BwRefresh);
        render::recovery_seed_observed(
            &mut self.display,
            &mut self.book,
            page,
            &mut buffers,
            &mut self.delay,
            &mut observer,
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
                let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::Probe);
                render::render_absolute_gray_observed(
                    &mut self.display,
                    &mut self.book,
                    page,
                    &mut buffers,
                    &mut self.delay,
                    &mut observer,
                )
                .await
            }
            xteink_x4_display::probes::ProbeKind::ClearWhite => {
                let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::Probe);
                xteink_x4_display::probes::clear_white_observed(
                    &mut self.display,
                    &mut self.delay,
                    &mut observer,
                )
                .await
            }
            xteink_x4_display::probes::ProbeKind::WindowCorners => {
                let mut observer = RuntimeBusyWaitObserver::new(BusyWaitSite::Probe);
                xteink_x4_display::probes::window_corners_observed(
                    &mut self.display,
                    &mut self.delay,
                    &mut observer,
                )
                .await
            }
        }
        #[cfg(not(feature = "diagnostic-console"))]
        {
            let _ = (kind, page);
            Err(xteink_x4_display::DisplayError::InvalidState)
        }
    }
}
