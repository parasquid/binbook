//! GRAY2 framebuffer with `embedded-graphics` `DrawTarget` support.
//!
//! Stores packed GRAY2 pixels in a 96 KB buffer (480×800 logical, 2 bits/pixel,
//! 4 pixels/byte, row-major MSB-first). This is the runtime rendering surface
//! for the library menu and other UI content.
//!
//! Canonical GRAY2 values: `Black=0`, `DarkGray=1`, `LightGray=2`, `White=3`.

use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{raw::RawU2, PixelColor},
    prelude::RawData,
    primitives::Rectangle,
    Pixel,
};

use crate::profile::{LOGICAL_HEIGHT, LOGICAL_WIDTH};

/// Total byte size of the GRAY2 framebuffer (480×800 ÷ 4 px/byte).
pub const FRAMEBUFFER_BYTES: usize =
    (LOGICAL_WIDTH as usize) * (LOGICAL_HEIGHT as usize) / 4;

/// A 2-bit gray pixel value matching the BinBook canonical GRAY2 encoding.
///
/// | Value | Name       |
/// |-------|------------|
/// | 0     | Black      |
/// | 1     | Dark gray  |
/// | 2     | Light gray |
/// | 3     | White      |
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Gray2Color(u8);

impl Gray2Color {
    pub const BLACK: Self = Self(0);
    pub const DARK_GRAY: Self = Self(1);
    pub const LIGHT_GRAY: Self = Self(2);
    pub const WHITE: Self = Self(3);

    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value & 0x03)
    }

    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl PixelColor for Gray2Color {
    type Raw = RawU2;
}

impl From<Gray2Color> for RawU2 {
    fn from(color: Gray2Color) -> Self {
        RawU2::new(color.0)
    }
}

impl From<RawU2> for Gray2Color {
    fn from(raw: RawU2) -> Self {
        Self(raw.into_inner() & 0x03)
    }
}

/// A packed GRAY2 framebuffer for the Xteink X4 logical display (480×800).
///
/// Pixels are stored row-major, MSB-first: the first pixel of each row occupies
/// bits 7-6 of the first byte, the fourth pixel occupies bits 1-0.
pub struct Gray2Framebuffer {
    data: [u8; FRAMEBUFFER_BYTES],
}

impl Gray2Framebuffer {
    /// Create a new framebuffer initialized to white (all pixels = 3).
    pub fn new() -> Self {
        Self {
            data: [0xff; FRAMEBUFFER_BYTES],
        }
    }

    /// Clear the entire framebuffer to a single color.
    pub fn clear(&mut self, color: Gray2Color) {
        let byte = match color.0 {
            0 => 0x00, // Black   → 00_00_00_00
            1 => 0x55, // Dark    → 01_01_01_01
            2 => 0xaa, // Light   → 10_10_10_10
            _ => 0xff, // White   → 11_11_11_11
        };
        self.data.fill(byte);
    }

    /// Return the packed byte at a given pixel position.
    ///
    /// `x` and `y` are in logical (480×800) coordinates.
    /// Returns the packed byte and the bit-shift offset (0, 2, 4, or 6) for
    /// the 2-bit pixel within that byte.
    #[must_use]
    pub fn pixel_offset(x: u16, y: u16) -> (usize, u8) {
        let logical_index = usize::from(y) * usize::from(LOGICAL_WIDTH) + usize::from(x);
        let byte_index = logical_index / 4;
        let shift = 6 - (logical_index % 4) * 2;
        #[allow(clippy::cast_possible_truncation)]
        let shift: u8 = shift as u8;
        (byte_index, shift)
    }

    /// Get the [`Gray2Color`] at a logical pixel position.
    #[must_use]
    pub fn get_pixel(&self, x: u16, y: u16) -> Gray2Color {
        let (index, shift) = Self::pixel_offset(x, y);
        Gray2Color::new((self.data[index] >> shift) & 0x03)
    }

    /// Set the [`Gray2Color`] at a logical pixel position.
    pub fn set_pixel(&mut self, x: u16, y: u16, color: Gray2Color) {
        let (index, shift) = Self::pixel_offset(x, y);
        let mask = 0x03 << shift;
        self.data[index] = (self.data[index] & !mask) | (color.0 << shift);
    }

    /// Return a reference to a packed GRAY2 row (480 pixels = 120 bytes).
    #[must_use]
    pub fn row(&self, y: u16) -> &[u8] {
        let start = usize::from(y) * (usize::from(LOGICAL_WIDTH) / 4);
        &self.data[start..start + (usize::from(LOGICAL_WIDTH) / 4)]
    }

    /// Return a mutable reference to a packed GRAY2 row.
    pub fn row_mut(&mut self, y: u16) -> &mut [u8] {
        let start = usize::from(y) * (usize::from(LOGICAL_WIDTH) / 4);
        &mut self.data[start..start + (usize::from(LOGICAL_WIDTH) / 4)]
    }

    /// Return the full packed framebuffer as a byte slice for the renderer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Return the full packed framebuffer as a mutable byte slice.
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

impl Default for Gray2Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for Gray2Framebuffer {
    fn size(&self) -> Size {
        Size::new(u32::from(LOGICAL_WIDTH), u32::from(LOGICAL_HEIGHT))
    }
}

impl DrawTarget for Gray2Framebuffer {
    type Color = Gray2Color;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x >= 0
                && (point.x as u16) < LOGICAL_WIDTH
                && point.y >= 0
                && (point.y as u16) < LOGICAL_HEIGHT
            {
                self.set_pixel(point.x as u16, point.y as u16, color);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        // Manual clip to visible area
        let x0 = area.top_left.x.max(0) as u16;
        let y0 = area.top_left.y.max(0) as u16;
        let x1 = (area.top_left.x + area.size.width as i32).min(i32::from(LOGICAL_WIDTH)).max(0) as u16;
        let y1 = (area.top_left.y + area.size.height as i32).min(i32::from(LOGICAL_HEIGHT)).max(0) as u16;
        let w = x1.saturating_sub(x0);
        let h = y1.saturating_sub(y0);

        let mut pixel_iter = pixels.into_iter();
        for y in y0..y0.saturating_add(h) {
            if y >= LOGICAL_HEIGHT {
                break;
            }
            for x in x0..x0.saturating_add(w) {
                if x >= LOGICAL_WIDTH {
                    break;
                }
                if let Some(color) = pixel_iter.next() {
                    self.set_pixel(x, y, color);
                }
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let x0 = area.top_left.x.max(0) as u16;
        let y0 = area.top_left.y.max(0) as u16;
        let x1 = (area.top_left.x + area.size.width as i32)
            .min(i32::from(LOGICAL_WIDTH))
            .max(0) as u16;
        let y1 = (area.top_left.y + area.size.height as i32)
            .min(i32::from(LOGICAL_HEIGHT))
            .max(0) as u16;
        let w = x1.saturating_sub(x0);

        for y in y0..y1 {
            if y >= LOGICAL_HEIGHT {
                break;
            }
            for x in x0..x0.saturating_add(w) {
                if x >= LOGICAL_WIDTH {
                    break;
                }
                self.set_pixel(x, y, color);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
use embedded_graphics::{
        geometry::Point,
        prelude::{Drawable, Primitive},
        primitives::PrimitiveStyle,
    };

    #[test]
    fn draw_target_writes_packed_gray2() {
        let mut fb = Gray2Framebuffer::new();
        fb.clear(Gray2Color::WHITE);

        // Draw a horizontal line of 4 black pixels at (0,0)-(3,0)
        Rectangle::new(Point::new(0, 0), Size::new(4, 1))
            .into_styled(PrimitiveStyle::with_stroke(
                Gray2Color::BLACK,
                1,
            ))
            .draw(&mut fb)
            .unwrap();

        // 4 black pixels at row 0 = 0b00_00_00_00 in one GRAY2-packed byte (MSB-first)
        assert_eq!(fb.row(0)[0], 0b00_00_00_00);
    }

    #[test]
    fn new_framebuffer_is_white() {
        let fb = Gray2Framebuffer::new();
        assert_eq!(fb.get_pixel(0, 0), Gray2Color::WHITE);
        assert_eq!(fb.get_pixel(479, 799), Gray2Color::WHITE);
        assert_eq!(fb.data.len(), FRAMEBUFFER_BYTES);
        // Every byte should be 0xff (white)
        assert!(fb.data.iter().all(|&b| b == 0xff));
    }

    #[test]
    fn set_and_get_pixel() {
        let mut fb = Gray2Framebuffer::new();
        fb.set_pixel(100, 200, Gray2Color::BLACK);
        assert_eq!(fb.get_pixel(100, 200), Gray2Color::BLACK);
        // Surrounding pixels should still be white
        assert_eq!(fb.get_pixel(99, 200), Gray2Color::WHITE);
        assert_eq!(fb.get_pixel(101, 200), Gray2Color::WHITE);
    }

    #[test]
    fn clear_to_dark_gray() {
        let mut fb = Gray2Framebuffer::new();
        fb.clear(Gray2Color::DARK_GRAY);
        assert_eq!(fb.get_pixel(0, 0), Gray2Color::DARK_GRAY);
        assert_eq!(fb.get_pixel(479, 799), Gray2Color::DARK_GRAY);
        assert_eq!(fb.data[0], 0x55);
        assert_eq!(fb.data[FRAMEBUFFER_BYTES - 1], 0x55);
    }

    #[test]
    fn pixel_offset_correctness() {
        // Pixel (0,0) → first byte, bits 7-6
        let (index, shift) = Gray2Framebuffer::pixel_offset(0, 0);
        assert_eq!(index, 0);
        assert_eq!(shift, 6);

        // Pixel (3,0) → first byte, bits 1-0
        let (index, shift) = Gray2Framebuffer::pixel_offset(3, 0);
        assert_eq!(index, 0);
        assert_eq!(shift, 0);

        // Pixel (4,0) → second byte, bits 7-6
        let (index, shift) = Gray2Framebuffer::pixel_offset(4, 0);
        assert_eq!(index, 1);
        assert_eq!(shift, 6);
    }

    #[test]
    fn row_returns_correct_slice() {
        let fb = Gray2Framebuffer::new();
        assert_eq!(fb.row(0).len(), usize::from(LOGICAL_WIDTH) / 4);
        assert_eq!(fb.row(1).len(), usize::from(LOGICAL_WIDTH) / 4);
        // Row 0 should start at data[0]; Row 1 at data[120]
        let row0_start = fb.row(0).as_ptr();
        let row1_start = fb.row(1).as_ptr();
        assert_eq!(
            row1_start as usize - row0_start as usize,
            usize::from(LOGICAL_WIDTH) / 4
        );
    }

    #[test]
    fn fill_solid_rectangle() {
        let mut fb = Gray2Framebuffer::new();
        let rect = Rectangle::new(Point::new(10, 10), Size::new(20, 30));
        fb.fill_solid(&rect, Gray2Color::BLACK).unwrap();

        // Inside: black
        for y in 10..40 {
            for x in 10..30 {
                assert_eq!(fb.get_pixel(x, y), Gray2Color::BLACK);
            }
        }
        // Outside unchanged: white
        assert_eq!(fb.get_pixel(9, 10), Gray2Color::WHITE);
        assert_eq!(fb.get_pixel(30, 10), Gray2Color::WHITE);
        assert_eq!(fb.get_pixel(10, 9), Gray2Color::WHITE);
        assert_eq!(fb.get_pixel(10, 40), Gray2Color::WHITE);
    }

    #[test]
    fn all_gray_values_round_trip() {
        let mut fb = Gray2Framebuffer::new();
        let values = [
            Gray2Color::BLACK,
            Gray2Color::DARK_GRAY,
            Gray2Color::LIGHT_GRAY,
            Gray2Color::WHITE,
        ];
        for (i, &color) in values.iter().enumerate() {
            let x = i as u16;
            fb.set_pixel(x, 0, color);
            assert_eq!(fb.get_pixel(x, 0), color, "value {i} round-trip failed");
        }
    }
}
