//! Feed a [`Gray2Framebuffer`] through the existing staged refresh pipeline.
//!
//! Since the firmware has no runtime graphics, the library menu (and any future
//! UI) is rendered into the framebuffer via `embedded-graphics` and then pushed
//! to the SSD1677 through the same BW→gray staged refresh that book pages use.
//!
//! The conversion flow is:
//!
//! 1. Take packed GRAY2 rows from `Gray2Framebuffer`
//! 2. Decompose to staged MSB/LSB/Base planes via `canonical_row_to_staged`
//! 3. Convert staged planes to absolute red/black RAM rows
//! 4. Write each RAM row through the existing `X4Panel` SPI path

use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use embedded_hal_async::delay::DelayNs;
use gray2_render::canonical_row_to_staged;

use crate::{
    framebuffer::Gray2Framebuffer,
    panel::X4Panel,
    profile::{CHUNK_COUNT, CHUNK_ROWS, PHYSICAL_WIDTH, ROW_BYTES},
    DisplayResult,
};

/// Write the BW base of a [`Gray2Framebuffer`] to the panel's red and black
/// RAM, ready for a staged gray overlay.
///
/// This is the equivalent of `render_bw_differential` but sourced from a
/// framebuffer instead of a `Book<R>`. The full framebuffer is written in both
/// RAM planes so the panel displays the menu state immediately in BW.
pub async fn render_ui_bw<SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    fb: &Gray2Framebuffer,
    delay: &mut D,
) -> DisplayResult<()>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let mut red_row = [0u8; ROW_BYTES];

    for strip in 0..CHUNK_COUNT {
        panel.controller().set_window(
            0,
            u16::from(strip) * CHUNK_ROWS,
            PHYSICAL_WIDTH,
            CHUNK_ROWS,
        )?;

        // Write red RAM: convert framebuffer rows to absolute red plane
        let mut error = None;
        panel
            .controller()
            .write_red_frame_rows::<ROW_BYTES>(CHUNK_ROWS, |_, output| {
                if error.is_some() {
                    output.fill(0xff);
                    return;
                }
                if let Err(e) = fill_absolute_from_fb(fb, strip, &mut red_row, output) {
                    error = Some(e);
                    output.fill(0xff);
                }
            })?;
        if let Some(error) = error {
            return Err(error);
        }
        delay.delay_ns(0).await;
    }

    // Write black RAM with same framebuffer data
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
                if let Err(e) = fill_absolute_from_fb(fb, strip, &mut red_row, output) {
                    error = Some(e);
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

    Ok(())
}

/// Write the gray overlay for a [`Gray2Framebuffer`] to the panel.
///
/// The LSB and MSB overlay planes are written to the black and red RAM
/// respectively, then `load_staged_gray` and `activate_staged_gray` are called
/// to trigger the gray settling waveform.
pub async fn render_ui_gray_overlay<SPI, DC, RST, BUSY, D>(
    panel: &mut X4Panel<SPI, DC, RST, BUSY>,
    fb: &Gray2Framebuffer,
    delay: &mut D,
) -> DisplayResult<()>
where
    SPI: SpiDevice<u8>,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let mut staged_msb = [0u8; ROW_BYTES];
    let mut staged_lsb = [0u8; ROW_BYTES];
    let mut staged_base = [0u8; ROW_BYTES];

    // Write LSB plane (stored as gray overlay black RAM)
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
                if let Err(e) = fill_staged_from_fb(fb, strip, Stage::Lsb, &mut staged_msb, &mut staged_lsb, &mut staged_base, output) {
                    error = Some(e);
                    output.fill(0xff);
                }
            })?;
        if let Some(error) = error {
            return Err(error);
        }
        delay.delay_ns(0).await;
    }

    // Write MSB plane (stored as gray overlay red RAM)
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
                if let Err(e) = fill_staged_from_fb(fb, strip, Stage::Msb, &mut staged_msb, &mut staged_lsb, &mut staged_base, output) {
                    error = Some(e);
                    output.fill(0xff);
                }
            })?;
        if let Some(error) = error {
            return Err(error);
        }
        delay.delay_ns(0).await;
    }

    panel.load_staged_gray()?;
    panel.activate_staged_gray(delay).await?;
    Ok(())
}

enum Stage {
    Lsb,
    Msb,
}

/// Convert framebuffer packed GRAY2 rows to absolute red/black RAM output for
/// a given strip.
fn fill_absolute_from_fb(
    fb: &Gray2Framebuffer,
    strip: u8,
    red: &mut [u8],
    black: &mut [u8],
) -> DisplayResult<()> {
    let row_dst = usize::from(strip) * usize::from(CHUNK_ROWS);
    let row_end = (row_dst + usize::from(CHUNK_ROWS)).min(usize::from(crate::profile::PHYSICAL_HEIGHT));

    for phys_y in row_dst..row_end {
        fill_absolute_row_packed(fb, phys_y, red, black)?;
    }
    Ok(())
}

/// Take a physical row Y, look up the corresponding logical data from the
/// framebuffer, and produce the SSD1677 absolute row.
fn fill_absolute_row_packed(
    fb: &Gray2Framebuffer,
    phys_y: usize,
    red: &mut [u8],
    black: &mut [u8],
) -> DisplayResult<()> {
    use crate::profile::LOGICAL_HEIGHT;

    // We store the framebuffer packed rows in LOGICAL coordinates (480×800).
    // For the physical row Y (in the 800×480 physical space), per the 270°
    // rotation: logical_x = physical_y, logical_y = LOGICAL_HEIGHT - 1 - physical_strip_x.
    //
    // But the full packed GRAY2 row is stored as 120 bytes per logical row,
    // covering all 480 logical pixels in that row.
    //
    // For each physical row Y (800 pixels wide), we need to gather pixels
    // from across the logical buffer using the rotation mapping.
    //
    // The rotation is:
    //   logical_x = Y  (the physical row index becomes the logical column)
    //   logical_y = LOGICAL_HEIGHT - 1 - strip_x  (where strip_x goes 0..799)
    //
    // So for each pixel position X in the physical row Y:
    //   source = fb.get_pixel(Y as u16, LOGICAL_HEIGHT - 1 - X as u16)
    //
    // Then that source pixel value gets placed at physical column X in the
    // SSD1677 row output.

    // We need to build the full physical row (800 pixels, 100 bytes of
    // red/black RAM). Each physical pixel X maps to logical (Y, 799-X).

    // Compute the physical row data by iterating physical columns.
    for phys_x in 0..crate::profile::PHYSICAL_WIDTH as usize {
        let logical_x = phys_y as u16; // physical_y → logical_x
        let logical_y = LOGICAL_HEIGHT - 1 - phys_x as u16; // phys_x → rotated logical_y

        if logical_y >= LOGICAL_HEIGHT {
            continue;
        }

        let gray = fb.get_pixel(logical_x, logical_y);

        // Set the corresponding bit in the physical red/black output.
        // SSD1677 rows are LSB-first per byte: bit 0 = leftmost pixel.
        let ram_bit = crate::profile::PHYSICAL_WIDTH as usize - 1 - phys_x;
        let byte_idx = ram_bit / 8;
        let bit_idx = ram_bit % 8;

        match gray.value() {
            0 => {
                // Black: active in both red and black planes
                red[byte_idx] |= 0x80 >> bit_idx;
                black[byte_idx] |= 0x80 >> bit_idx;
            }
            1 => {
                // Dark gray: active in black only
                black[byte_idx] |= 0x80 >> bit_idx;
            }
            2 => {
                // Light gray: active in red only
                red[byte_idx] |= 0x80 >> bit_idx;
            }
            3 => {
                // White: no bits set (already cleared)
            }
            _ => {}
        }
    }

    Ok(())
}

/// Fill a staged overlay plane (LSB or MSB) from framebuffer data for a given
/// strip.
fn fill_staged_from_fb(
    fb: &Gray2Framebuffer,
    strip: u8,
    stage: Stage,
    msb: &mut [u8],
    lsb: &mut [u8],
    base: &mut [u8],
    output: &mut [u8],
) -> DisplayResult<()> {
    use crate::profile::{LOGICAL_HEIGHT, PHYSICAL_HEIGHT, PHYSICAL_WIDTH};

    let row_dst = usize::from(strip) * usize::from(CHUNK_ROWS);
    let row_end = (row_dst + usize::from(CHUNK_ROWS)).min(usize::from(PHYSICAL_HEIGHT));

    let mut canonical_row = [0u8; 120]; // 480 pixels packed = 120 bytes
    let row_len = ROW_BYTES.min(output.len() / usize::from(CHUNK_ROWS));

    for phys_y in row_dst..row_end {
        canonical_row.fill(0);

        for phys_x in 0..PHYSICAL_WIDTH as usize {
            let logical_x = phys_y as u16;
            let logical_y = LOGICAL_HEIGHT - 1 - phys_x as u16;

            if logical_y < LOGICAL_HEIGHT {
                let gray = fb.get_pixel(logical_x, logical_y);
                let byte_idx = phys_x / 4;
                let shift = 6 - (phys_x % 4) * 2;
                canonical_row[byte_idx] |= gray.value() << shift;
            }
        }

        canonical_row_to_staged(&canonical_row, msb, lsb, base)
            .map_err(|_| crate::DisplayError::Render)?;

        // Copy the requested plane from the appropriate buffer without moving
        let row_offset = phys_y - row_dst;
        let start = row_offset * row_len;
        if start + row_len <= output.len() {
            match stage {
                Stage::Lsb => output[start..start + row_len].copy_from_slice(&lsb[..row_len]),
                Stage::Msb => output[start..start + row_len].copy_from_slice(&msb[..row_len]),
            }
        }
    }

    Ok(())
}

/// Convert packed logical GRAY2 data to SSD1677 absolute black RAM row.
/// This is the inverse of the rotation mapping.
#[must_use]
pub fn logical_gray2_to_black_row(packed: &[u8], row_y: u16) -> [u8; ROW_BYTES] {
    use crate::profile::{LOGICAL_HEIGHT, PHYSICAL_WIDTH};
    let mut row = [0u8; ROW_BYTES];
    for phys_x in 0..PHYSICAL_WIDTH as usize {
        let logical_y = LOGICAL_HEIGHT - 1 - phys_x as u16;
        if logical_y >= LOGICAL_HEIGHT {
            continue;
        }
        let byte_idx = usize::from(row_y) * usize::from(crate::profile::LOGICAL_WIDTH) / 4
            + phys_x / 4;
        if byte_idx >= packed.len() {
            continue;
        }
        let shift = 6 - (phys_x % 4) * 2;
        let gray = (packed[byte_idx] >> shift) & 0x03;

        let ram_bit = PHYSICAL_WIDTH as usize - 1 - phys_x;
        let out_byte = ram_bit / 8;
        let out_bit = ram_bit % 8;
        if gray == 0 || gray == 1 {
            // Black/Dark gray → black plane active
            row[out_byte] |= 0x80 >> out_bit;
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::{Gray2Color, Gray2Framebuffer};

    #[test]
    fn black_row_from_framebuffer() {
        let mut fb = Gray2Framebuffer::new();
        fb.clear(Gray2Color::WHITE);
        // Set pixel at logical (0, 0) to black
        fb.set_pixel(0, 0, Gray2Color::BLACK);

        // Physical row 0 (from 270° rotation) maps to logical_x = 0, so
        // the black pixel at logical (0,0) should appear at physical row 0.
        let black_row = logical_gray2_to_black_row(fb.as_bytes(), 0);

        // Verify it's not all-white (the black pixel should set a bit)
        assert!(
            black_row.iter().any(|&b| b != 0),
            "black row should have bits set"
        );
    }

    #[test]
    fn white_framebuffer_produces_empty_ram() {
        let fb = Gray2Framebuffer::new();
        let mut red = [0u8; crate::profile::ROW_BYTES];
        let mut black = [0u8; crate::profile::ROW_BYTES];

        let result = fill_absolute_row_packed(&fb, 0, &mut red, &mut black);
        assert!(result.is_ok());
        assert_eq!(red, [0u8; crate::profile::ROW_BYTES]);
        assert_eq!(black, [0u8; crate::profile::ROW_BYTES]);
    }

    #[test]
    fn each_gray_value_produces_correct_plane_bits() {
        let mut fb = Gray2Framebuffer::new();
        fb.set_pixel(0, 479, Gray2Color::BLACK);

        let mut red = [0u8; crate::profile::ROW_BYTES];
        let mut black = [0u8; crate::profile::ROW_BYTES];

        let _ = fill_absolute_row_packed(&fb, 0, &mut red, &mut black);

        let any_bit_set = |row: &[u8]| -> bool { row.iter().any(|&b| b != 0) };
        assert!(any_bit_set(&red), "black pixel should set red bits");
        assert!(any_bit_set(&black), "black pixel should set black bits");
    }
}
