use ssd1677_driver::Ssd1677Driver;
use xteink_hal::{AsyncDelay, HalError, HalResult, InputPin, OutputPin, RefreshMode, Spi};

use crate::refresh::{RefreshDecision, RefreshPolicy, RefreshState, X4_CHUNK_COUNT};

pub type EmbeddedBook<'a> = binbook_core::Book<binbook_core::SliceSource<'a>>;

fn read_page(book: &mut EmbeddedBook<'_>, raw: u32) -> HalResult<binbook_core::PageInfo> {
    let number = book.page_number(raw).map_err(|_| HalError::InvalidParam)?;
    let mut record = [0_u8; binbook_core::PAGE_RECORD_SIZE];
    book.page(number, &mut record)
        .map_err(|_| HalError::InvalidParam)
}

fn read_profile(book: &mut EmbeddedBook<'_>) -> HalResult<binbook_core::DisplayProfile> {
    let mut record = [0_u8; 56];
    book.display_profile(&mut record)
        .map_err(|_| HalError::InvalidParam)
}

fn read_transition(
    book: &mut EmbeddedBook<'_>,
    raw: u32,
) -> HalResult<binbook_core::PageTransition> {
    let number = book
        .transition_number(raw)
        .map_err(|_| HalError::InvalidParam)?;
    let mut record = [0_u8; 24];
    book.transition(number, &mut record)
        .map_err(|_| HalError::InvalidParam)
}

fn plane_descriptor(
    planes: &binbook_core::PlaneDirectory,
    raw: usize,
) -> HalResult<binbook_core::PlaneDescriptor> {
    let slot = u8::try_from(raw)
        .ok()
        .and_then(|value| binbook_core::PlaneSlot::try_from(value).ok())
        .ok_or(HalError::InvalidParam)?;
    planes.get(slot).ok_or(HalError::InvalidParam)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Unknown,
    Grayscale,
    Bw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrayRenderOutcome {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseSyncOutcome {
    Completed,
    Cancelled,
}

fn ensure_grayscale_mode<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    if *panel_mode != PanelMode::Grayscale {
        display.init_grayscale_with_delay(delay)?;
        *panel_mode = PanelMode::Grayscale;
    }
    Ok(())
}

fn ensure_bw_mode<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    if *panel_mode != PanelMode::Bw {
        display.init_with_delay(delay)?;
        *panel_mode = PanelMode::Bw;
    }
    Ok(())
}

pub const GRAY1_ROW_BYTES: usize = 60;
pub const GRAY2_ROW_BYTES: usize = 200;
pub const DISPLAY_ROW_BYTES: usize = 100;
pub const PAGE_WIDTH: u16 = 480;
pub const PAGE_HEIGHT: u16 = 800;
pub const DISPLAY_WIDTH: u16 = 800;
pub const DISPLAY_HEIGHT: u16 = 480;
pub const PROBE_BOX_WIDTH: u16 = 128;
pub const PROBE_BOX_HEIGHT: u16 = 96;
pub const X4_CHUNK_ROWS: u16 = 16;

pub fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    (PAGE_HEIGHT - 1 - logical_y, logical_x)
}

pub fn smoke_probe_windows() -> [(u16, u16, u16, u16); 4] {
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

pub fn build_display_smoke_row(row: u16, row_buf: &mut [u8; DISPLAY_ROW_BYTES]) {
    row_buf.fill(0xFF);

    if row < PROBE_BOX_HEIGHT || row >= DISPLAY_HEIGHT - PROBE_BOX_HEIGHT {
        let probe_byte_width = usize::from(PROBE_BOX_WIDTH / 8);
        row_buf[..probe_byte_width].fill(0x00);
        row_buf[DISPLAY_ROW_BYTES - probe_byte_width..].fill(0x00);
    }
}

pub fn display_page<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;

    stream_gray1_rows(compressed_data, DISPLAY_HEIGHT, |row, row_buf| {
        display.write_row(row, row_buf)
    })?;

    display.refresh_with_delay(RefreshMode::Partial, &NoDelay)
}

pub fn display_gray2_page<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_gray2_plane(display, compressed_data, true)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_gray2_plane(display, compressed_data, false)?;
    display.refresh_with_delay(RefreshMode::Grayscale, delay)
}

pub async fn display_gray2_page_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_gray2_plane_strips_async(display, compressed_data, true, true, delay).await?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_gray2_plane_strips_async(display, compressed_data, false, false, delay).await?;
    display.refresh_async(RefreshMode::Grayscale, delay).await
}

pub async fn display_full_grayscale_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();
    let pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    let msb = compressed_native_plane(book_bytes, page_data_offset, pd, 0)?;
    let lsb = compressed_native_plane(book_bytes, page_data_offset, pd, 1)?;
    let base = compressed_native_plane(book_bytes, page_data_offset, pd, 2)?;
    let strip_count = DISPLAY_HEIGHT / X4_CHUNK_ROWS;

    let mut msb_decoder = PackBitsCursor::new(msb);
    let mut lsb_decoder = PackBitsCursor::new(lsb);
    let mut base_decoder = PackBitsCursor::new(base);
    let mut msb_row = [0u8; DISPLAY_ROW_BYTES];
    let mut lsb_row = [0u8; DISPLAY_ROW_BYTES];
    let mut base_row = [0u8; DISPLAY_ROW_BYTES];
    let mut unused_black = [0u8; DISPLAY_ROW_BYTES];
    for strip in 0..strip_count {
        display.set_window(0, strip * X4_CHUNK_ROWS, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row| {
            msb_decoder.fill(&mut msb_row);
            lsb_decoder.fill(&mut lsb_row);
            base_decoder.fill(&mut base_row);
            let converted = gray2_render::staged_row_to_absolute(
                &msb_row,
                &lsb_row,
                &base_row,
                row,
                &mut unused_black,
            );
            debug_assert!(converted.is_ok());
        })?;
        delay.ms(0).await;
    }

    let mut lsb_decoder = PackBitsCursor::new(lsb);
    let mut base_decoder = PackBitsCursor::new(base);
    let mut unused_red = [0u8; DISPLAY_ROW_BYTES];
    for strip in 0..strip_count {
        display.set_window(0, strip * X4_CHUNK_ROWS, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        display.write_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row| {
            lsb_decoder.fill(&mut lsb_row);
            base_decoder.fill(&mut base_row);
            let converted = gray2_render::staged_row_to_absolute(
                &msb_row,
                &lsb_row,
                &base_row,
                &mut unused_red,
                row,
            );
            debug_assert!(converted.is_ok());
        })?;
        if strip + 1 < strip_count {
            delay.ms(0).await;
        }
    }
    display.refresh_async(RefreshMode::Grayscale, delay).await
}

fn compressed_native_plane<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    slot: usize,
) -> HalResult<&'a [u8]> {
    let descriptor = plane_descriptor(pd, slot)?;
    let start = usize::try_from(page_data_offset + descriptor.offset.get())
        .map_err(|_| HalError::InvalidParam)?;
    let end = start
        .checked_add(usize::try_from(descriptor.length.get()).map_err(|_| HalError::InvalidParam)?)
        .ok_or(HalError::InvalidParam)?;
    book_bytes.get(start..end).ok_or(HalError::InvalidParam)
}

pub async fn display_staged_grayscale_async<SPI, CS, DC, RST, BUSY, D, E, A>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    expected_epoch: u32,
    mut request_epoch: E,
    mut on_activate: A,
    delay: &D,
) -> HalResult<GrayRenderOutcome>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
    E: FnMut() -> u32,
    A: FnMut(),
{
    let page = validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();

    if !stream_plane_chunks_cancellable_async(
        display,
        book_bytes,
        page_data_offset,
        &page.planes,
        1,
        false,
        expected_epoch,
        &mut request_epoch,
        delay,
    )
    .await?
    {
        return Ok(GrayRenderOutcome::Cancelled);
    }
    if !stream_plane_chunks_cancellable_async(
        display,
        book_bytes,
        page_data_offset,
        &page.planes,
        0,
        true,
        expected_epoch,
        &mut request_epoch,
        delay,
    )
    .await?
    {
        return Ok(GrayRenderOutcome::Cancelled);
    }
    if request_epoch() != expected_epoch {
        return Ok(GrayRenderOutcome::Cancelled);
    }
    display.load_staged_grayscale_lut()?;
    if request_epoch() != expected_epoch {
        return Ok(GrayRenderOutcome::Cancelled);
    }
    on_activate();
    display.activate_staged_grayscale_async(delay).await?;
    Ok(GrayRenderOutcome::Completed)
}

pub async fn sync_bw_base_async<SPI, CS, DC, RST, BUSY, D, E>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    expected_epoch: u32,
    mut request_epoch: E,
    delay: &D,
) -> HalResult<BaseSyncOutcome>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
    E: FnMut() -> u32,
{
    let page = validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();
    let completed = stream_plane_chunks_cancellable_async(
        display,
        book_bytes,
        page_data_offset,
        &page.planes,
        2,
        true,
        expected_epoch,
        &mut request_epoch,
        delay,
    )
    .await?;
    Ok(if completed {
        BaseSyncOutcome::Completed
    } else {
        BaseSyncOutcome::Cancelled
    })
}

pub async fn bw_differential_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    prev_page: u32,
    target_page: u32,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    validate_x4_native_page(book, prev_page)?;
    validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();
    let prev_pd = &read_page(book, prev_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;
    let target_pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_strips_async(
        display,
        book_bytes,
        page_data_offset,
        prev_pd,
        2,
        true,
        true,
        delay,
    )
    .await?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_strips_async(
        display,
        book_bytes,
        page_data_offset,
        target_pd,
        2,
        false,
        false,
        delay,
    )
    .await?;
    display.refresh_async(RefreshMode::Partial, delay).await
}

pub async fn recovery_seed_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();
    let pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_strips_async(
        display,
        book_bytes,
        page_data_offset,
        pd,
        2,
        true,
        true,
        delay,
    )
    .await?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_strips_async(
        display,
        book_bytes,
        page_data_offset,
        pd,
        2,
        false,
        false,
        delay,
    )
    .await?;
    display.refresh_async(RefreshMode::Full, delay).await
}

#[cfg(feature = "diagnostic-console")]
pub async fn clear_white_probe_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xFF))?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xFF))?;
    display.refresh_async(RefreshMode::Full, delay).await
}

#[cfg(feature = "diagnostic-console")]
pub async fn window_corners_probe_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xFF))?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row| row.fill(0xFF))?;
    for &(x, y, width, height) in &smoke_probe_windows() {
        display.write_solid_window(x, y, width, height, 0x00)?;
    }
    for &(x, y, width, height) in &smoke_probe_windows() {
        display.write_red_solid_window(x, y, width, height, 0x00)?;
    }
    display.refresh_async(RefreshMode::Full, delay).await
}

fn stream_gray2_plane<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
    red_plane: bool,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let mut decoder = PackBitsCursor::new(compressed_data);
    let mut gray2_row = [0u8; GRAY2_ROW_BYTES];
    let mut red = [0u8; DISPLAY_ROW_BYTES];
    let mut black = [0u8; DISPLAY_ROW_BYTES];

    if red_plane {
        display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |row, row_buf| {
            let _ = row;
            gray2_row.fill(0);
            decoder.fill(&mut gray2_row);
            let converted =
                gray2_render::canonical_row_to_absolute(&gray2_row, &mut red, &mut black);
            debug_assert!(converted.is_ok());
            row_buf.copy_from_slice(&red);
        })
    } else {
        display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |row, row_buf| {
            let _ = row;
            gray2_row.fill(0);
            decoder.fill(&mut gray2_row);
            let converted =
                gray2_render::canonical_row_to_absolute(&gray2_row, &mut red, &mut black);
            debug_assert!(converted.is_ok());
            row_buf.copy_from_slice(&black);
        })
    }
}

async fn stream_gray2_plane_strips_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
    red_plane: bool,
    yield_after_last_strip: bool,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    let mut decoder = PackBitsCursor::new(compressed_data);
    let mut gray2_row = [0u8; GRAY2_ROW_BYTES];
    let mut red = [0u8; DISPLAY_ROW_BYTES];
    let mut black = [0u8; DISPLAY_ROW_BYTES];
    let strip_count = DISPLAY_HEIGHT / X4_CHUNK_ROWS;

    for strip in 0..strip_count {
        let y = strip * X4_CHUNK_ROWS;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;

        if red_plane {
            display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |row, row_buf| {
                let _ = row;
                gray2_row.fill(0);
                decoder.fill(&mut gray2_row);
                let converted =
                    gray2_render::canonical_row_to_absolute(&gray2_row, &mut red, &mut black);
                debug_assert!(converted.is_ok());
                row_buf.copy_from_slice(&red);
            })?;
        } else {
            display.write_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |row, row_buf| {
                let _ = row;
                gray2_row.fill(0);
                decoder.fill(&mut gray2_row);
                let converted =
                    gray2_render::canonical_row_to_absolute(&gray2_row, &mut red, &mut black);
                debug_assert!(converted.is_ok());
                row_buf.copy_from_slice(&black);
            })?;
        }

        if strip + 1 < strip_count || yield_after_last_strip {
            delay.ms(0).await;
        }
    }

    Ok(())
}

async fn stream_native_plane_strips_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    compressed_data: &[u8],
    red_plane: bool,
    yield_after_last_strip: bool,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    let mut decoder = PackBitsCursor::new(compressed_data);
    let strip_count = DISPLAY_HEIGHT / X4_CHUNK_ROWS;

    for strip in 0..strip_count {
        let y = strip * X4_CHUNK_ROWS;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;

        if red_plane {
            display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row_buf| {
                row_buf.fill(0xFF);
                decoder.fill(row_buf);
            })?;
        } else {
            display.write_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row_buf| {
                row_buf.fill(0xFF);
                decoder.fill(row_buf);
            })?;
        }

        if strip + 1 < strip_count || yield_after_last_strip {
            delay.ms(0).await;
        }
    }

    Ok(())
}

async fn stream_plane_chunks_strips_async<SPI, CS, DC, RST, BUSY, D>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    red_plane: bool,
    yield_after_last_strip: bool,
    delay: &D,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let abs = page_data_offset + descriptor.offset.get();
    let start = usize::try_from(abs).map_err(|_| HalError::InvalidParam)?;
    let end = start
        .checked_add(usize::try_from(descriptor.length.get()).map_err(|_| HalError::InvalidParam)?)
        .ok_or(HalError::InvalidParam)?;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..end];
    stream_native_plane_strips_async(
        display,
        compressed,
        red_plane,
        yield_after_last_strip,
        delay,
    )
    .await
}

async fn stream_plane_chunks_cancellable_async<SPI, CS, DC, RST, BUSY, D, E>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    red_plane: bool,
    expected_epoch: u32,
    request_epoch: &mut E,
    delay: &D,
) -> HalResult<bool>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: AsyncDelay + ?Sized,
    E: FnMut() -> u32,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let start = usize::try_from(page_data_offset + descriptor.offset.get())
        .map_err(|_| HalError::InvalidParam)?;
    let end = start
        .checked_add(usize::try_from(descriptor.length.get()).map_err(|_| HalError::InvalidParam)?)
        .ok_or(HalError::InvalidParam)?;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let mut decoder = PackBitsCursor::new(&book_bytes[start..end]);
    let strip_count = DISPLAY_HEIGHT / X4_CHUNK_ROWS;

    for strip in 0..strip_count {
        if request_epoch() != expected_epoch {
            return Ok(false);
        }
        let y = strip * X4_CHUNK_ROWS;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        if red_plane {
            display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row_buf| {
                row_buf.fill(0xFF);
                decoder.fill(row_buf);
            })?;
        } else {
            display.write_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |_, row_buf| {
                row_buf.fill(0xFF);
                decoder.fill(row_buf);
            })?;
        }
        delay.ms(0).await;
    }
    Ok(true)
}

pub fn stream_gray1_rows<E>(
    compressed_data: &[u8],
    row_count: u16,
    mut write_row: impl FnMut(u16, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let mut decoder = PackBitsCursor::new(compressed_data);
    let mut row_buf = [0u8; GRAY1_ROW_BYTES];

    for row in 0..row_count {
        row_buf.fill(0);
        decoder.fill(&mut row_buf);
        write_row(row, &row_buf)?;
    }

    Ok(())
}

pub fn stream_gray2_rows<E>(
    compressed_data: &[u8],
    row_count: u16,
    mut write_row: impl FnMut(u16, &[u8; GRAY2_ROW_BYTES]) -> Result<(), E>,
) -> Result<(), E> {
    let mut decoder = PackBitsCursor::new(compressed_data);
    let mut row_buf = [0u8; GRAY2_ROW_BYTES];

    for row in 0..row_count {
        row_buf.fill(0);
        decoder.fill(&mut row_buf);
        write_row(row, &row_buf)?;
    }

    Ok(())
}

pub fn decompress_row(input: &[u8], output: &mut [u8]) -> usize {
    let mut decoder = binbook_decompress::PackBitsDecoder::new();
    decoder
        .decode(input, output)
        .map_or(0, |progress| progress.consumed)
}

pub fn is_supported_embedded_gray2_page(page: &binbook_core::PageInfo) -> bool {
    page.pixel_format == binbook_core::PixelFormat::Gray2Packed
        && page.compression_method == binbook_core::CompressionMethod::RlePackBits
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && page.planes.bitmap() == 0x01
}

pub fn is_supported_x4_native_gray2_page(
    profile: &binbook_core::DisplayProfile,
    page: &binbook_core::PageInfo,
) -> bool {
    profile.physical_width == DISPLAY_WIDTH
        && profile.physical_height == DISPLAY_HEIGHT
        && profile.waveform_hint == binbook_core::WAVEFORM_SSD1677_STAGED_GRAY2
        && page.pixel_format == binbook_core::PixelFormat::Gray2Packed
        && page.compression_method == binbook_core::CompressionMethod::RlePackBits
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && page.planes.bitmap() == 0x07
}

fn validate_x4_native_page(
    book: &mut EmbeddedBook<'_>,
    page_number: u32,
) -> HalResult<binbook_core::PageInfo> {
    let profile = read_profile(book).map_err(|_| HalError::InvalidParam)?;
    let page = read_page(book, page_number).map_err(|_| HalError::InvalidParam)?;
    if !is_supported_x4_native_gray2_page(&profile, &page) {
        return Err(HalError::InvalidParam);
    }
    Ok(page)
}

pub fn embedded_page_slice<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    page: &binbook_core::PageInfo,
) -> Option<&'a [u8]> {
    if !is_supported_embedded_gray2_page(page) {
        return None;
    }
    let pd = &page.planes;
    let descriptor = pd.get(binbook_core::PlaneSlot::OverlayMsb)?;
    let offset = page_data_offset.checked_add(descriptor.offset.get())?;
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(descriptor.length.get()).ok()?;
    let end = start.checked_add(size)?;
    if end > book_bytes.len() {
        return None;
    }
    Some(&book_bytes[start..end])
}

pub fn embedded_chunk_slice<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    chunk: &binbook_core::PageChunk,
) -> Option<&'a [u8]> {
    let offset = page_data_offset.checked_add(chunk.offset.get())?;
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(chunk.compressed_length.get()).ok()?;
    let end = start.checked_add(size)?;
    if end > book_bytes.len() {
        return None;
    }
    Some(&book_bytes[start..end])
}

pub fn find_transition_mask(
    book: &mut EmbeddedBook<'_>,
    previous_page: Option<u32>,
    target_page: u32,
) -> Option<u32> {
    let prev = previous_page?;
    if prev == target_page {
        return None;
    }
    for i in 0..book.transition_count() {
        if let Ok(entry) = read_transition(book, i) {
            if entry.from.get() == prev && entry.to.get() == target_page {
                return Some(entry.changed_chunk_mask);
            }
        }
    }
    None
}

pub fn display_page_with_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    panel_mode: &mut PanelMode,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display_page_with_refresh_policy(
        display,
        book,
        book_bytes,
        delay,
        refresh_state,
        panel_mode,
        RefreshPolicy::FullScreenDifferentialDefault,
        target_page,
    )
}

pub fn display_page_with_chunk_dirty_probe_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    panel_mode: &mut PanelMode,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    display_page_with_refresh_policy(
        display,
        book,
        book_bytes,
        delay,
        refresh_state,
        panel_mode,
        RefreshPolicy::ChunkDirtyDifferentialDefault,
        target_page,
    )
}

pub fn display_page_with_refresh_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    refresh_state: &mut RefreshState,
    panel_mode: &mut PanelMode,
    refresh_policy: RefreshPolicy,
    target_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    validate_x4_native_page(book, target_page)?;

    let transition_mask = find_transition_mask(book, refresh_state.previous_page(), target_page);
    let decision = refresh_state.decide_with_policy(target_page, transition_mask, refresh_policy);

    match decision {
        RefreshDecision::Noop => return Ok(()),
        RefreshDecision::FullGrayscale => {
            ensure_grayscale_mode(display, delay, panel_mode)?;
            stream_full_grayscale(display, book, book_bytes, target_page, delay)?;
        }
        RefreshDecision::FullBwSeed => {
            ensure_bw_mode(display, delay, panel_mode)?;
            stream_bw_seed_full(display, book, book_bytes, target_page, delay)?;
        }
        RefreshDecision::AdjacentDirtyPartial { changed_chunk_mask } => {
            ensure_bw_mode(display, delay, panel_mode)?;
            let prev = refresh_state.previous_page().unwrap();
            stream_bw_differential_chunked(
                display,
                book,
                book_bytes,
                prev,
                target_page,
                changed_chunk_mask,
                delay,
            )?;
        }
        RefreshDecision::FullScreenDifferential => {
            ensure_bw_mode(display, delay, panel_mode)?;
            let prev = refresh_state.previous_page().unwrap();
            stream_bw_differential_full(display, book, book_bytes, prev, target_page, delay)?;
        }
    }

    refresh_state.record_success(target_page, decision);
    Ok(())
}

fn stream_full_grayscale<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    validate_x4_native_page(book, target_page)?;
    let page_data_offset = book.page_data_offset().get();
    let pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, page_data_offset, &pd, 0, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, page_data_offset, &pd, 1, delay)?;
    display.refresh_with_delay(RefreshMode::Grayscale, delay)
}

fn stream_bw_seed_full<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    target_page: u32,
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let page_data_offset = book.page_data_offset().get();
    let pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, page_data_offset, &pd, 2, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, page_data_offset, &pd, 2, delay)?;
    display.refresh_with_delay(RefreshMode::Full, delay)
}

fn stream_bw_differential_chunked<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    prev_page: u32,
    target_page: u32,
    changed_mask: u32,
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let page_data_offset = book.page_data_offset().get();
    let prev_pd = &read_page(book, prev_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;
    let target_pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    for chunk_idx in 0..X4_CHUNK_COUNT {
        if changed_mask & (1 << chunk_idx) == 0 {
            continue;
        }
        let y = chunk_idx as u16 * X4_CHUNK_ROWS;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        stream_single_chunk_to_red(display, book_bytes, page_data_offset, prev_pd, 2, chunk_idx)?;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        stream_single_chunk_to_black(
            display,
            book_bytes,
            page_data_offset,
            target_pd,
            2,
            chunk_idx,
        )?;
    }

    display.refresh_with_delay(RefreshMode::Partial, delay)
}

fn stream_bw_differential_full<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    prev_page: u32,
    target_page: u32,
    delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let page_data_offset = book.page_data_offset().get();
    let prev_pd = &read_page(book, prev_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;
    let target_pd = &read_page(book, target_page)
        .map_err(|_| HalError::InvalidParam)?
        .planes;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, page_data_offset, prev_pd, 2, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, page_data_offset, target_pd, 2, delay)?;
    display.refresh_with_delay(RefreshMode::Partial, delay)
}

fn stream_plane_chunks_to_red<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    _delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let abs = page_data_offset + descriptor.offset.get();
    let start = usize::try_from(abs).map_err(|_| HalError::InvalidParam)?;
    let end = start
        .checked_add(usize::try_from(descriptor.length.get()).map_err(|_| HalError::InvalidParam)?)
        .ok_or(HalError::InvalidParam)?;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..end];
    let mut decoder = PackBitsCursor::new(compressed);
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })
}

fn stream_plane_chunks_to_black<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    _delay: &dyn xteink_hal::Delay,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let abs = page_data_offset + descriptor.offset.get();
    let start = usize::try_from(abs).map_err(|_| HalError::InvalidParam)?;
    let end = start
        .checked_add(usize::try_from(descriptor.length.get()).map_err(|_| HalError::InvalidParam)?)
        .ok_or(HalError::InvalidParam)?;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..end];
    let mut decoder = PackBitsCursor::new(compressed);
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })
}

fn stream_compressed_row(compressed: &[u8], row: usize, row_buf: &mut [u8; DISPLAY_ROW_BYTES]) {
    let mut decoder = PackBitsCursor::new(compressed);
    let mut skip = [0u8; DISPLAY_ROW_BYTES];
    for _ in 0..row {
        decoder.fill(&mut skip);
    }
    row_buf.fill(0xFF);
    decoder.fill(row_buf);
}

fn stream_single_chunk_to_red<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    chunk_idx: u8,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let abs = page_data_offset + descriptor.offset.get();
    let start = usize::try_from(abs).map_err(|_| HalError::InvalidParam)?;
    if start >= book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..];
    let mut decoder = PackBitsCursor::new(compressed);
    let mut skip = [0u8; DISPLAY_ROW_BYTES];
    let skip_rows = chunk_idx as usize * X4_CHUNK_ROWS as usize;
    for _ in 0..skip_rows {
        decoder.fill(&mut skip);
    }
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |row, row_buf| {
        let _ = row;
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })?;
    Ok(())
}

fn stream_single_chunk_to_black<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook_core::PlaneDirectory,
    plane_slot: usize,
    chunk_idx: u8,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    let descriptor = plane_descriptor(pd, plane_slot)?;
    let abs = page_data_offset + descriptor.offset.get();
    let start = usize::try_from(abs).map_err(|_| HalError::InvalidParam)?;
    if start >= book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..];
    let mut decoder = PackBitsCursor::new(compressed);
    let mut skip = [0u8; DISPLAY_ROW_BYTES];
    let skip_rows = chunk_idx as usize * X4_CHUNK_ROWS as usize;
    for _ in 0..skip_rows {
        decoder.fill(&mut skip);
    }
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(X4_CHUNK_ROWS, |row, row_buf| {
        let _ = row;
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })?;
    Ok(())
}

struct NoDelay;

impl xteink_hal::Delay for NoDelay {
    fn ms(&self, _ms: u32) {}
}

#[derive(Debug, Clone, Copy)]
struct PackBitsCursor<'a> {
    input: &'a [u8],
    pos: usize,
    decoder: binbook_decompress::PackBitsDecoder,
}

impl<'a> PackBitsCursor<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            decoder: binbook_decompress::PackBitsDecoder::new(),
        }
    }

    fn fill(&mut self, output: &mut [u8]) {
        let mut produced = 0;
        while produced < output.len() {
            let Ok(progress) = self
                .decoder
                .decode(&self.input[self.pos..], &mut output[produced..])
            else {
                break;
            };
            self.pos += progress.consumed;
            produced += progress.produced;
            if progress.consumed == 0 && progress.produced == 0 {
                break;
            }
        }
    }
}

#[cfg(feature = "diagnostic-console")]
pub fn display_full_refresh_current<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut EmbeddedBook<'_>,
    book_bytes: &[u8],
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
    current_page: u32,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    ensure_grayscale_mode(display, delay, panel_mode)?;
    stream_full_grayscale(display, book, book_bytes, current_page, delay)
}

#[cfg(feature = "diagnostic-console")]
pub fn display_clear_white_probe<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    ensure_bw_mode(display, delay, panel_mode)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
    })?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
    })?;
    display.refresh_with_delay(RefreshMode::Full, delay)
}

#[cfg(feature = "diagnostic-console")]
pub fn display_window_corners_probe<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    delay: &dyn xteink_hal::Delay,
    panel_mode: &mut PanelMode,
) -> HalResult<()>
where
    SPI: Spi,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
{
    ensure_bw_mode(display, delay, panel_mode)?;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
    })?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
    })?;

    let corners = smoke_probe_windows();
    for &(x, y, w, h) in &corners {
        display.write_solid_window(x, y, w, h, 0x00)?;
    }
    for &(x, y, w, h) in &corners {
        display.write_red_solid_window(x, y, w, h, 0x00)?;
    }

    display.refresh_with_delay(RefreshMode::Full, delay)
}
