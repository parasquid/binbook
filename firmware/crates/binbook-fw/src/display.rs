use ssd1677_driver::Ssd1677Driver;
use xteink_hal::{HalError, HalResult, InputPin, OutputPin, RefreshMode, Spi};

use crate::refresh::{RefreshDecision, RefreshPolicy, RefreshState, X4_CHUNK_COUNT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Unknown,
    Grayscale,
    Bw,
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
    let mut decoder = PackBitsStream::new(compressed_data);
    let mut gray2_row = [0u8; GRAY2_ROW_BYTES];
    let mut red = [0u8; DISPLAY_ROW_BYTES];
    let mut black = [0u8; DISPLAY_ROW_BYTES];

    if red_plane {
        display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |row, row_buf| {
            let _ = row;
            gray2_row.fill(0);
            decoder.fill(&mut gray2_row);
            gray2_row_to_ssd1677_planes(&gray2_row, &mut red, &mut black);
            row_buf.copy_from_slice(&red);
        })
    } else {
        display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |row, row_buf| {
            let _ = row;
            gray2_row.fill(0);
            decoder.fill(&mut gray2_row);
            gray2_row_to_ssd1677_planes(&gray2_row, &mut red, &mut black);
            row_buf.copy_from_slice(&black);
        })
    }
}

pub fn stream_gray1_rows<E>(
    compressed_data: &[u8],
    row_count: u16,
    mut write_row: impl FnMut(u16, &[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let mut decoder = PackBitsStream::new(compressed_data);
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
    let mut decoder = PackBitsStream::new(compressed_data);
    let mut row_buf = [0u8; GRAY2_ROW_BYTES];

    for row in 0..row_count {
        row_buf.fill(0);
        decoder.fill(&mut row_buf);
        write_row(row, &row_buf)?;
    }

    Ok(())
}

pub fn gray2_row_to_ssd1677_planes(
    gray2_row: &[u8; GRAY2_ROW_BYTES],
    red_row: &mut [u8; DISPLAY_ROW_BYTES],
    black_row: &mut [u8; DISPLAY_ROW_BYTES],
) {
    red_row.fill(0xFF);
    black_row.fill(0xFF);

    for (byte_index, packed) in gray2_row.iter().copied().enumerate() {
        let physical_x = (byte_index * 4) as u16;
        for pixel in 0..4 {
            let gray = (packed >> (6 - pixel * 2)) & 0x03;
            let xth = gray2_to_xteink_value(gray);
            let x = physical_x + pixel as u16;
            if xth & 0x02 != 0 {
                clear_ssd1677_pixel(red_row, x);
            }
            if xth & 0x01 != 0 {
                clear_ssd1677_pixel(black_row, x);
            }
        }
    }
}

fn gray2_to_xteink_value(gray: u8) -> u8 {
    match gray & 0x03 {
        0 => 3,
        1 => 2,
        2 => 1,
        _ => 0,
    }
}

fn clear_ssd1677_pixel(row: &mut [u8; DISPLAY_ROW_BYTES], physical_x: u16) {
    if physical_x >= DISPLAY_WIDTH {
        return;
    }
    let ram_x = DISPLAY_WIDTH - 1 - physical_x;
    row[usize::from(ram_x / 8)] &= !(0x80 >> (ram_x % 8));
}

pub fn decompress_row(input: &[u8], output: &mut [u8]) -> usize {
    let mut in_pos = 0;
    let mut out_pos = 0;

    while out_pos < output.len() && in_pos < input.len() {
        let control = input[in_pos];
        in_pos += 1;

        if control <= 127 {
            let requested = control as usize + 1;
            let copy_count = requested
                .min(output.len() - out_pos)
                .min(input.len().saturating_sub(in_pos));
            output[out_pos..out_pos + copy_count]
                .copy_from_slice(&input[in_pos..in_pos + copy_count]);
            out_pos += copy_count;
            in_pos += copy_count;
        } else {
            if in_pos >= input.len() {
                break;
            }

            let value = input[in_pos];
            in_pos += 1;

            let repeat_count = ((control & 0x7F) as usize + 1).min(output.len() - out_pos);
            output[out_pos..out_pos + repeat_count].fill(value);
            out_pos += repeat_count;
        }
    }

    in_pos
}

pub fn is_supported_embedded_gray2_page(page: &binbook::PageInfo) -> bool {
    page.pixel_format == binbook::page_index::PIXEL_FORMAT_GRAY2_PACKED
        && page.compression_method == binbook::page_index::COMPRESSION_RLE_PACKBITS
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && page.plane_dir.bitmap == 0x01
}

pub fn is_supported_x4_native_gray2_page(page: &binbook::PageInfo) -> bool {
    page.pixel_format == binbook::page_index::PIXEL_FORMAT_GRAY2_PACKED
        && page.compression_method == binbook::page_index::COMPRESSION_RLE_PACKBITS
        && page.stored_width == DISPLAY_WIDTH
        && page.stored_height == DISPLAY_HEIGHT
        && (page.plane_dir.bitmap & 0x07) == 0x07
}

pub fn embedded_page_slice<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    page: &binbook::PageInfo,
) -> Option<&'a [u8]> {
    if !is_supported_embedded_gray2_page(page) {
        return None;
    }
    let pd = &page.plane_dir;
    let offset = page_data_offset.checked_add(pd.offsets[0] as u64)?;
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(pd.sizes[0]).ok()?;
    let end = start.checked_add(size)?;
    if end > book_bytes.len() {
        return None;
    }
    Some(&book_bytes[start..end])
}

pub fn embedded_chunk_slice<'a>(
    book_bytes: &'a [u8],
    page_data_offset: u64,
    chunk: &binbook::chunk_index::PageChunkEntry,
) -> Option<&'a [u8]> {
    let offset = page_data_offset.checked_add(chunk.page_data_offset as u64)?;
    let start = usize::try_from(offset).ok()?;
    let size = usize::try_from(chunk.compressed_size).ok()?;
    let end = start.checked_add(size)?;
    if end > book_bytes.len() {
        return None;
    }
    Some(&book_bytes[start..end])
}

pub fn find_transition_mask(
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
    previous_page: Option<u32>,
    target_page: u32,
) -> Option<u32> {
    let prev = previous_page?;
    if prev == target_page {
        return None;
    }
    for i in 0..book.transition_count() {
        if let Ok(entry) = book.transition_entry(i) {
            if entry.from_page_number == prev && entry.to_page_number == target_page {
                return Some(entry.changed_chunk_mask);
            }
        }
    }
    None
}

pub fn display_page_with_policy<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    let page_info = book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?;
    if !is_supported_x4_native_gray2_page(&page_info) {
        return Err(HalError::InvalidParam);
    }

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
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    let open = book.open_info();
    let pd = &book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, open.page_data_offset, &pd, 0, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, open.page_data_offset, &pd, 1, delay)?;
    display.refresh_with_delay(RefreshMode::Grayscale, delay)
}

fn stream_bw_seed_full<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    let open = book.open_info();
    let pd = &book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(display, book_bytes, open.page_data_offset, &pd, 2, delay)?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(display, book_bytes, open.page_data_offset, &pd, 2, delay)?;
    display.refresh_with_delay(RefreshMode::Full, delay)
}

fn stream_bw_differential_chunked<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    let open = book.open_info();
    let prev_pd = &book
        .page_info(prev_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;
    let target_pd = &book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;

    for chunk_idx in 0..X4_CHUNK_COUNT {
        if changed_mask & (1 << chunk_idx) == 0 {
            continue;
        }
        let y = chunk_idx as u16 * X4_CHUNK_ROWS;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        stream_single_chunk_to_red(
            display,
            book_bytes,
            open.page_data_offset,
            prev_pd,
            2,
            chunk_idx,
        )?;
        display.set_window(0, y, DISPLAY_WIDTH, X4_CHUNK_ROWS)?;
        stream_single_chunk_to_black(
            display,
            book_bytes,
            open.page_data_offset,
            target_pd,
            2,
            chunk_idx,
        )?;
    }

    display.refresh_with_delay(RefreshMode::Partial, delay)
}

fn stream_bw_differential_full<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
    let open = book.open_info();
    let prev_pd = &book
        .page_info(prev_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;
    let target_pd = &book
        .page_info(target_page)
        .map_err(|_| HalError::InvalidParam)?
        .plane_dir;

    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_red(
        display,
        book_bytes,
        open.page_data_offset,
        prev_pd,
        2,
        delay,
    )?;
    display.set_window(0, 0, DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    stream_plane_chunks_to_black(
        display,
        book_bytes,
        open.page_data_offset,
        target_pd,
        2,
        delay,
    )?;
    display.refresh_with_delay(RefreshMode::Partial, delay)
}

fn stream_plane_chunks_to_red<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook::page_index::PlaneDir,
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
    let slot_offset = pd.offsets[plane_slot];
    let slot_size = pd.sizes[plane_slot];
    let abs = page_data_offset + slot_offset as u64;
    let start = abs as usize;
    let end = start + slot_size as usize;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..end];
    let mut decoder = PackBitsStream::new(compressed);
    display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })
}

fn stream_plane_chunks_to_black<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book_bytes: &[u8],
    page_data_offset: u64,
    pd: &binbook::page_index::PlaneDir,
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
    let slot_offset = pd.offsets[plane_slot];
    let slot_size = pd.sizes[plane_slot];
    let abs = page_data_offset + slot_offset as u64;
    let start = abs as usize;
    let end = start + slot_size as usize;
    if end > book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..end];
    let mut decoder = PackBitsStream::new(compressed);
    display.write_frame_rows::<DISPLAY_ROW_BYTES>(DISPLAY_HEIGHT, |_, row_buf| {
        row_buf.fill(0xFF);
        decoder.fill(row_buf);
    })
}

fn stream_compressed_row(compressed: &[u8], row: usize, row_buf: &mut [u8; DISPLAY_ROW_BYTES]) {
    let mut decoder = PackBitsStream::new(compressed);
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
    pd: &binbook::page_index::PlaneDir,
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
    let slot_offset = pd.offsets[plane_slot];
    let abs = page_data_offset + slot_offset as u64;
    let start = abs as usize;
    if start >= book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..];
    let mut decoder = PackBitsStream::new(compressed);
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
    pd: &binbook::page_index::PlaneDir,
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
    let slot_offset = pd.offsets[plane_slot];
    let abs = page_data_offset + slot_offset as u64;
    let start = abs as usize;
    if start >= book_bytes.len() {
        return Err(HalError::InvalidParam);
    }
    let compressed = &book_bytes[start..];
    let mut decoder = PackBitsStream::new(compressed);
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
enum Run {
    Literal { remaining: usize },
    Repeat { value: u8, remaining: usize },
}

#[derive(Debug, Clone, Copy)]
struct PackBitsStream<'a> {
    input: &'a [u8],
    pos: usize,
    run: Option<Run>,
}

impl<'a> PackBitsStream<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            run: None,
        }
    }

    fn fill(&mut self, output: &mut [u8]) {
        let mut out_pos = 0;

        while out_pos < output.len() {
            if self.run.is_none() && !self.load_next_run() {
                break;
            }

            match self.run {
                Some(Run::Literal { remaining }) => {
                    let count = remaining
                        .min(output.len() - out_pos)
                        .min(self.input.len().saturating_sub(self.pos));
                    output[out_pos..out_pos + count]
                        .copy_from_slice(&self.input[self.pos..self.pos + count]);
                    self.pos += count;
                    out_pos += count;
                    self.run = update_literal_run(remaining, count);
                    if count == 0 {
                        break;
                    }
                }
                Some(Run::Repeat { value, remaining }) => {
                    let count = remaining.min(output.len() - out_pos);
                    output[out_pos..out_pos + count].fill(value);
                    out_pos += count;
                    self.run = update_repeat_run(value, remaining, count);
                }
                None => {}
            }
        }
    }

    fn load_next_run(&mut self) -> bool {
        if self.pos >= self.input.len() {
            return false;
        }

        let control = self.input[self.pos];
        self.pos += 1;

        if control <= 127 {
            self.run = Some(Run::Literal {
                remaining: control as usize + 1,
            });
            true
        } else if self.pos < self.input.len() {
            let value = self.input[self.pos];
            self.pos += 1;
            self.run = Some(Run::Repeat {
                value,
                remaining: (control & 0x7F) as usize + 1,
            });
            true
        } else {
            false
        }
    }
}

fn update_literal_run(remaining: usize, consumed: usize) -> Option<Run> {
    remaining
        .checked_sub(consumed)
        .filter(|&remaining| remaining > 0)
        .map(|remaining| Run::Literal { remaining })
}

fn update_repeat_run(value: u8, remaining: usize, consumed: usize) -> Option<Run> {
    remaining
        .checked_sub(consumed)
        .filter(|&remaining| remaining > 0)
        .map(|remaining| Run::Repeat { value, remaining })
}

#[cfg(feature = "diagnostic-console")]
pub fn display_full_refresh_current<SPI, CS, DC, RST, BUSY>(
    display: &mut Ssd1677Driver<SPI, CS, DC, RST, BUSY>,
    book: &mut binbook::BinBook<&[u8], &mut [u8; 8192]>,
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
        display.set_window(x, y, w, h)?;
        display.write_frame_rows::<DISPLAY_ROW_BYTES>(h, |_, row_buf| {
            row_buf.fill(0x00);
        })?;
    }
    for &(x, y, w, h) in &corners {
        display.set_window(x, y, w, h)?;
        display.write_red_frame_rows::<DISPLAY_ROW_BYTES>(h, |_, row_buf| {
            row_buf.fill(0x00);
        })?;
    }

    display.refresh_with_delay(RefreshMode::Full, delay)
}
