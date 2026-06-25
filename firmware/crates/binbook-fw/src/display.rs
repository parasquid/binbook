use ssd1677_driver::Ssd1677Driver;
use xteink_hal::{HalResult, InputPin, OutputPin, RefreshMode, Spi};

pub const GRAY1_ROW_BYTES: usize = 60;
pub const DISPLAY_ROW_BYTES: usize = 100;
pub const PAGE_WIDTH: u16 = 480;
pub const PAGE_HEIGHT: u16 = 800;
pub const DISPLAY_WIDTH: u16 = 800;
pub const DISPLAY_HEIGHT: u16 = 480;

pub fn logical_to_physical(logical_x: u16, logical_y: u16) -> (u16, u16) {
    (PAGE_HEIGHT - 1 - logical_y, logical_x)
}

pub fn build_display_smoke_row(row: u16, row_buf: &mut [u8; DISPLAY_ROW_BYTES]) {
    row_buf.fill(0xFF);

    if row < 96 {
        row_buf[..16].fill(0x00);
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
