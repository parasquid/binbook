use binbook_core::{Book, PlaneSlot, ReadAt};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use embedded_hal_async::delay::DelayNs;
use ssd1677_driver::{BusyWaitObserver, NoopBusyWaitObserver};

use crate::{
    buffers::{first_input, first_row, RenderBuffers},
    page_source::{read_x4_page, required_plane},
    panel::{RefreshMode, X4Panel},
    plane_write::{write_plane, write_plane_timed, PlaneWrite},
    render_timing::{
        elapsed_u32, NoopRenderTimingObserver, PlaneRole, RamTarget, RenderObservers,
        RenderStageStatus, RenderTimingObserver,
    },
    DisplayResult,
};

pub async fn sync_bw_base<R, SPI, DC, RST, BUSY, D, E>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    page: u32,
    buffers: &mut RenderBuffers<'_>,
    expected_epoch: u32,
    mut epoch: E,
    delay: &mut D,
) -> DisplayResult<crate::events::OperationOutcome>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    E: FnMut() -> u32,
{
    let page = read_x4_page(book, page)?;
    let completed = write_plane(
        panel,
        book,
        required_plane(page.planes, PlaneSlot::FastBase)?,
        PlaneWrite::new(
            first_input(buffers.compressed)?,
            first_row(buffers.decoded)?,
            RamTarget::Red,
            expected_epoch,
            &mut epoch,
            delay,
        ),
    )
    .await?;
    Ok(if completed {
        crate::events::OperationOutcome::Completed
    } else {
        crate::events::OperationOutcome::Cancelled
    })
}

pub async fn render_bw_differential<R, SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    from: u32,
    target: u32,
    buffers: &mut RenderBuffers<'_>,
    delay: &mut D,
) -> DisplayResult<()>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    render_bw_differential_observed(
        panel,
        book,
        from,
        target,
        buffers,
        delay,
        &mut NoopBusyWaitObserver,
    )
    .await
}

pub async fn render_bw_differential_observed<R, SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    from: u32,
    target: u32,
    buffers: &mut RenderBuffers<'_>,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> DisplayResult<()>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let mut timing = NoopRenderTimingObserver;
    render_bw_differential_timed(
        panel,
        book,
        from,
        target,
        buffers,
        delay,
        RenderObservers {
            busy: observer,
            timing: &mut timing,
        },
    )
    .await
}

pub async fn render_bw_differential_timed<R, SPI, DC, RST, BUSY, D, B, T>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    from: u32,
    target: u32,
    buffers: &mut RenderBuffers<'_>,
    delay: &mut D,
    observers: RenderObservers<'_, B, T>,
) -> DisplayResult<()>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    B: BusyWaitObserver,
    T: RenderTimingObserver,
{
    let timing_observer = observers.timing;
    let metadata_start = timing_observer.now_ms();
    let from_page = from;
    let target_page = target;
    let from = read_x4_page(book, from_page)?;
    let target = read_x4_page(book, target_page)?;
    let metadata_duration = elapsed_u32(metadata_start, timing_observer.now_ms());
    timing_observer.page_metadata_read(from_page, target_page, metadata_duration);
    let input = first_input(buffers.compressed)?;
    let row = first_row(buffers.decoded)?;
    let mut epoch = || 0;
    write_plane_timed(
        panel,
        book,
        required_plane(from.planes, PlaneSlot::FastBase)?,
        PlaneWrite::new(input, row, RamTarget::Red, 0, &mut epoch, delay),
        PlaneRole::PreviousFastBase,
        timing_observer,
    )
    .await?;
    write_plane_timed(
        panel,
        book,
        required_plane(target.planes, PlaneSlot::FastBase)?,
        PlaneWrite::new(input, row, RamTarget::Black, 0, &mut epoch, delay),
        PlaneRole::TargetFastBase,
        timing_observer,
    )
    .await?;
    let trigger_start = timing_observer.now_ms();
    let trigger_result = panel.controller().trigger_refresh(RefreshMode::Partial);
    let trigger_duration = elapsed_u32(trigger_start, timing_observer.now_ms());
    timing_observer.refresh_trigger(
        RefreshMode::Partial,
        trigger_duration,
        if trigger_result.is_ok() {
            RenderStageStatus::Ok
        } else {
            RenderStageStatus::Error
        },
    );
    trigger_result?;
    panel.wait_ready_observed(delay, observers.busy).await?;
    Ok(())
}

pub async fn recovery_seed<R, SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    page: u32,
    buffers: &mut RenderBuffers<'_>,
    delay: &mut D,
) -> DisplayResult<()>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    recovery_seed_observed(panel, book, page, buffers, delay, &mut NoopBusyWaitObserver).await
}

pub async fn recovery_seed_observed<R, SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    page: u32,
    buffers: &mut RenderBuffers<'_>,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> DisplayResult<()>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let page = read_x4_page(book, page)?;
    let input = first_input(buffers.compressed)?;
    let row = first_row(buffers.decoded)?;
    let plane = required_plane(page.planes, PlaneSlot::FastBase)?;
    let mut epoch = || 0;
    write_plane(
        panel,
        book,
        plane,
        PlaneWrite::new(input, row, RamTarget::Red, 0, &mut epoch, delay),
    )
    .await?;
    write_plane(
        panel,
        book,
        plane,
        PlaneWrite::new(input, row, RamTarget::Black, 0, &mut epoch, delay),
    )
    .await?;
    panel
        .refresh_observed(RefreshMode::Full, delay, observer)
        .await?;
    Ok(())
}
