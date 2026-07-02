use binbook_core::{Book, ReadAt};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use embedded_hal_async::delay::DelayNs;

use crate::{
    page_source::PlaneDecoder,
    panel::X4Panel,
    profile::{CHUNK_COUNT, CHUNK_ROWS, PHYSICAL_WIDTH, ROW_BYTES},
    render_timing::{
        elapsed_u32, NoopRenderTimingObserver, PlaneRole, RamTarget, RenderStageStatus,
        RenderTimingObserver,
    },
    DisplayResult,
};

pub(crate) async fn write_plane<R, SPI, DC, RST, BUSY, D, E>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    plane: binbook_core::PlaneDescriptor,
    write: PlaneWrite<'_, E, D>,
) -> DisplayResult<bool>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    E: FnMut() -> u32,
{
    write_plane_timed(
        panel,
        book,
        plane,
        write,
        PlaneRole::PreviousFastBase,
        &mut NoopRenderTimingObserver,
    )
    .await
}

pub(crate) async fn write_plane_timed<R, SPI, DC, RST, BUSY, D, E>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    plane: binbook_core::PlaneDescriptor,
    write: PlaneWrite<'_, E, D>,
    role: PlaneRole,
    timing_observer: &mut impl RenderTimingObserver,
) -> DisplayResult<bool>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    E: FnMut() -> u32,
{
    let plane_start = timing_observer.now_ms();
    timing_observer.plane_write_start(role, write.target, plane.length.get());
    let mut decoder = PlaneDecoder::new(plane);
    let mut fill_ms = 0u32;
    let mut spi_ms = 0u32;
    let mut rows = 0u32;
    for strip in 0..CHUNK_COUNT {
        if (write.epoch)() != write.expected_epoch {
            let duration = elapsed_u32(plane_start, timing_observer.now_ms());
            timing_observer.plane_write_end(role, duration, RenderStageStatus::Cancelled);
            return Ok(false);
        }
        panel.controller().set_window(
            0,
            u16::from(strip) * CHUNK_ROWS,
            PHYSICAL_WIDTH,
            CHUNK_ROWS,
        )?;
        let mut error = None;
        let write_start = timing_observer.now_ms();
        let fill_before = fill_ms;
        let mut fill = |_: u16, output: &mut [u8; ROW_BYTES]| {
            let fill_start = timing_observer.now_ms();
            if let Err(value) = decoder.fill(book, write.input, write.row) {
                error = Some(value);
                output.fill(0xff);
            } else {
                output.copy_from_slice(write.row);
            }
            fill_ms = fill_ms.saturating_add(elapsed_u32(fill_start, timing_observer.now_ms()));
            rows = rows.saturating_add(1);
        };
        if write.target == RamTarget::Red {
            panel
                .controller()
                .write_red_frame_rows(CHUNK_ROWS, &mut fill)?;
        } else {
            panel.controller().write_frame_rows(CHUNK_ROWS, &mut fill)?;
        }
        let write_duration = elapsed_u32(write_start, timing_observer.now_ms());
        spi_ms = spi_ms.saturating_add(write_duration.saturating_sub(fill_ms - fill_before));
        if let Some(error) = error {
            let duration = elapsed_u32(plane_start, timing_observer.now_ms());
            timing_observer.plane_write_end(role, duration, RenderStageStatus::Error);
            return Err(error);
        }
        write.delay.delay_ns(0).await;
    }
    decoder.finish()?;
    timing_observer.plane_row_fill_summary(role, fill_ms, rows);
    timing_observer.plane_spi_write_summary(role, spi_ms, rows.saturating_mul(ROW_BYTES as u32));
    let duration = elapsed_u32(plane_start, timing_observer.now_ms());
    timing_observer.plane_write_end(role, duration, RenderStageStatus::Ok);
    Ok(true)
}

pub(crate) struct PlaneWrite<'a, E, D> {
    pub(crate) input: &'a mut [u8],
    pub(crate) row: &'a mut [u8],
    pub(crate) target: RamTarget,
    pub(crate) expected_epoch: u32,
    pub(crate) epoch: &'a mut E,
    pub(crate) delay: &'a mut D,
}

impl<'a, E, D> PlaneWrite<'a, E, D> {
    pub(crate) fn new(
        input: &'a mut [u8],
        row: &'a mut [u8],
        target: RamTarget,
        expected_epoch: u32,
        epoch: &'a mut E,
        delay: &'a mut D,
    ) -> Self {
        Self {
            input,
            row,
            target,
            expected_epoch,
            epoch,
            delay,
        }
    }
}
