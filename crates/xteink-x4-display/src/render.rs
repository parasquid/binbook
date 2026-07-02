use binbook_core::{Book, PlaneSlot, ReadAt};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use embedded_hal_async::delay::DelayNs;
use ssd1677_driver::{BusyWaitObserver, NoopBusyWaitObserver};

use crate::{
    buffers::{first_row, require_row, row_triplet, split_three, RenderBuffers},
    events::OperationOutcome,
    native::{write_plane, PlaneWrite, RamTarget},
    page_source::{read_x4_page, required_plane, PlaneDecoder},
    panel::{RefreshMode, X4Panel},
    profile::{CHUNK_COUNT, CHUNK_ROWS, PHYSICAL_WIDTH, ROW_BYTES},
    DisplayResult,
};

pub async fn render_absolute_gray<R, SPI, DC, RST, BUSY, D>(
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
    render_absolute_gray_observed(panel, book, page, buffers, delay, &mut NoopBusyWaitObserver)
        .await
}

pub async fn render_absolute_gray_observed<R, SPI, DC, RST, BUSY, D>(
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
    let (input0, input1, input2) = split_three(buffers.compressed)?;
    let (row0, row1, row2) = row_triplet(buffers.decoded)?;
    require_row(buffers.red)?;
    require_row(buffers.black)?;
    let mut msb = PlaneDecoder::new(required_plane(page.planes, PlaneSlot::OverlayMsb)?);
    let mut lsb = PlaneDecoder::new(required_plane(page.planes, PlaneSlot::OverlayLsb)?);
    let mut base = PlaneDecoder::new(required_plane(page.planes, PlaneSlot::FastBase)?);
    for strip in 0..CHUNK_COUNT {
        panel.controller().set_window(
            0,
            u16::from(strip) * CHUNK_ROWS,
            PHYSICAL_WIDTH,
            CHUNK_ROWS,
        )?;
        let mut error = None;
        panel
            .controller()
            .write_red_frame_rows::<ROW_BYTES>(CHUNK_ROWS, |_, output| {
                if error.is_some() {
                    output.fill(0xff);
                    return;
                }
                if let Err(value) = fill_absolute_row(
                    book,
                    (&mut msb, input0, row0),
                    (&mut lsb, input1, row1),
                    (&mut base, input2, row2),
                    output,
                    buffers.black,
                ) {
                    error = Some(value);
                    output.fill(0xff);
                }
            })?;
        if let Some(error) = error {
            return Err(error);
        }
        delay.delay_ns(0).await;
    }
    msb.finish()?;
    lsb.finish()?;
    base.finish()?;

    let mut lsb = PlaneDecoder::new(required_plane(page.planes, PlaneSlot::OverlayLsb)?);
    let mut base = PlaneDecoder::new(required_plane(page.planes, PlaneSlot::FastBase)?);
    row0.fill(0);
    for strip in 0..CHUNK_COUNT {
        panel.controller().set_window(
            0,
            u16::from(strip) * CHUNK_ROWS,
            PHYSICAL_WIDTH,
            CHUNK_ROWS,
        )?;
        let mut error = None;
        panel
            .controller()
            .write_frame_rows::<ROW_BYTES>(CHUNK_ROWS, |_, output| {
                if error.is_some() {
                    output.fill(0xff);
                    return;
                }
                let result = lsb
                    .fill(book, input1, row1)
                    .and_then(|()| base.fill(book, input2, row2))
                    .and_then(|()| {
                        gray2_render::staged_row_to_absolute(row0, row1, row2, buffers.red, output)
                            .map_err(Into::into)
                    });
                if let Err(value) = result {
                    error = Some(value);
                    output.fill(0xff);
                }
            })?;
        if let Some(error) = error {
            return Err(error);
        }
        if strip + 1 < CHUNK_COUNT {
            delay.delay_ns(0).await;
        }
    }
    lsb.finish()?;
    base.finish()?;
    panel
        .refresh_observed(RefreshMode::Grayscale, delay, observer)
        .await?;
    Ok(())
}

pub struct OverlayControl<E, A> {
    pub expected_epoch: u32,
    pub epoch: E,
    pub on_activate: A,
}

pub async fn render_staged_overlay<R, SPI, DC, RST, BUSY, D, E, A>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    page: u32,
    buffers: &mut RenderBuffers<'_>,
    control: OverlayControl<E, A>,
    delay: &mut D,
) -> DisplayResult<OperationOutcome>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    E: FnMut() -> u32,
    A: FnMut(),
{
    render_staged_overlay_observed(
        panel,
        book,
        page,
        buffers,
        control,
        delay,
        &mut NoopBusyWaitObserver,
    )
    .await
}

pub async fn render_staged_overlay_observed<R, SPI, DC, RST, BUSY, D, E, A>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    book: &mut Book<R>,
    page: u32,
    buffers: &mut RenderBuffers<'_>,
    mut control: OverlayControl<E, A>,
    delay: &mut D,
    observer: &mut impl BusyWaitObserver,
) -> DisplayResult<OperationOutcome>
where
    R: ReadAt,
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
    E: FnMut() -> u32,
    A: FnMut(),
{
    let page = read_x4_page(book, page)?;
    let (_, input1, input2) = split_three(buffers.compressed)?;
    let row = first_row(buffers.decoded)?;
    if !write_plane(
        panel,
        book,
        required_plane(page.planes, PlaneSlot::OverlayLsb)?,
        PlaneWrite::new(
            input1,
            row,
            RamTarget::Black,
            control.expected_epoch,
            &mut control.epoch,
            delay,
        ),
    )
    .await?
    {
        return Ok(OperationOutcome::Cancelled);
    }
    if !write_plane(
        panel,
        book,
        required_plane(page.planes, PlaneSlot::OverlayMsb)?,
        PlaneWrite::new(
            input2,
            row,
            RamTarget::Red,
            control.expected_epoch,
            &mut control.epoch,
            delay,
        ),
    )
    .await?
    {
        return Ok(OperationOutcome::Cancelled);
    }
    if (control.epoch)() != control.expected_epoch {
        return Ok(OperationOutcome::Cancelled);
    }
    panel.load_staged_gray()?;
    if (control.epoch)() != control.expected_epoch {
        return Ok(OperationOutcome::Cancelled);
    }
    (control.on_activate)();
    panel.activate_staged_gray_observed(delay, observer).await?;
    Ok(OperationOutcome::Completed)
}

fn fill_absolute_row<R: ReadAt>(
    book: &mut Book<R>,
    msb: (&mut PlaneDecoder, &mut [u8], &mut [u8]),
    lsb: (&mut PlaneDecoder, &mut [u8], &mut [u8]),
    base: (&mut PlaneDecoder, &mut [u8], &mut [u8]),
    red: &mut [u8],
    black: &mut [u8],
) -> DisplayResult<()> {
    msb.0.fill(book, msb.1, msb.2)?;
    lsb.0.fill(book, lsb.1, lsb.2)?;
    base.0.fill(book, base.1, base.2)?;
    gray2_render::staged_row_to_absolute(msb.2, lsb.2, base.2, red, black)?;
    Ok(())
}

pub use crate::native::{
    recovery_seed, recovery_seed_observed, render_bw_differential, render_bw_differential_observed,
    sync_bw_base,
};
