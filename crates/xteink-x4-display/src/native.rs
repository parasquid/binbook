use binbook_core::{Book, PlaneSlot, ReadAt};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use embedded_hal_async::delay::DelayNs;
use ssd1677_driver::{BusyWaitObserver, NoopBusyWaitObserver};

use crate::{
    buffers::{first_input, first_row, RenderBuffers},
    page_source::{read_x4_page, required_plane, PlaneDecoder},
    panel::{RefreshMode, X4Panel},
    profile::{CHUNK_COUNT, CHUNK_ROWS, PHYSICAL_WIDTH, ROW_BYTES},
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
    let from = read_x4_page(book, from)?;
    let target = read_x4_page(book, target)?;
    let input = first_input(buffers.compressed)?;
    let row = first_row(buffers.decoded)?;
    let mut epoch = || 0;
    write_plane(
        panel,
        book,
        required_plane(from.planes, PlaneSlot::FastBase)?,
        PlaneWrite::new(input, row, RamTarget::Red, 0, &mut epoch, delay),
    )
    .await?;
    write_plane(
        panel,
        book,
        required_plane(target.planes, PlaneSlot::FastBase)?,
        PlaneWrite::new(input, row, RamTarget::Black, 0, &mut epoch, delay),
    )
    .await?;
    panel
        .refresh_observed(RefreshMode::Partial, delay, observer)
        .await?;
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
    let mut decoder = PlaneDecoder::new(plane);
    for strip in 0..CHUNK_COUNT {
        if (write.epoch)() != write.expected_epoch {
            return Ok(false);
        }
        panel.controller().set_window(
            0,
            u16::from(strip) * CHUNK_ROWS,
            PHYSICAL_WIDTH,
            CHUNK_ROWS,
        )?;
        let mut error = None;
        let mut fill = |_: u16, output: &mut [u8; ROW_BYTES]| {
            if let Err(value) = decoder.fill(book, write.input, write.row) {
                error = Some(value);
                output.fill(0xff);
            } else {
                output.copy_from_slice(write.row);
            }
        };
        if write.target == RamTarget::Red {
            panel
                .controller()
                .write_red_frame_rows(CHUNK_ROWS, &mut fill)?;
        } else {
            panel.controller().write_frame_rows(CHUNK_ROWS, &mut fill)?;
        }
        if let Some(error) = error {
            return Err(error);
        }
        write.delay.delay_ns(0).await;
    }
    decoder.finish()?;
    Ok(true)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RamTarget {
    Black,
    Red,
}

pub(crate) struct PlaneWrite<'a, E, D> {
    input: &'a mut [u8],
    row: &'a mut [u8],
    target: RamTarget,
    expected_epoch: u32,
    epoch: &'a mut E,
    delay: &'a mut D,
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
