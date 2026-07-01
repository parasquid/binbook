use crate::RenderError;

#[must_use]
pub const fn quantize_gray1(luma: u8) -> u8 {
    if luma < 128 {
        0
    } else {
        1
    }
}

#[must_use]
pub const fn quantize_gray2(luma: u8) -> u8 {
    match luma {
        0..=42 => 0,
        43..=127 => 1,
        128..=212 => 2,
        213..=255 => 3,
    }
}

#[derive(Debug, PartialEq)]
pub struct FloydSteinberg<'a> {
    width: usize,
    current: &'a mut [f32],
    next: &'a mut [f32],
}

impl<'a> FloydSteinberg<'a> {
    pub fn new(
        width: usize,
        current: &'a mut [f32],
        next: &'a mut [f32],
    ) -> Result<Self, RenderError> {
        if width == 0 {
            return Err(RenderError::InvalidDimensions);
        }
        let required = width.checked_add(2).ok_or(RenderError::InvalidDimensions)?;
        require(current, required)?;
        require(next, required)?;
        current[..required].fill(0.0);
        next[..required].fill(0.0);
        Ok(Self {
            width,
            current: &mut current[..required],
            next: &mut next[..required],
        })
    }

    pub fn quantize_gray1_row(
        &mut self,
        luma: &[u8],
        output: &mut [u8],
    ) -> Result<(), RenderError> {
        self.quantize_row(luma, output, &[0.0, 255.0])
    }

    pub fn quantize_gray2_row(
        &mut self,
        luma: &[u8],
        output: &mut [u8],
    ) -> Result<(), RenderError> {
        self.quantize_row(luma, output, &[0.0, 85.0, 170.0, 255.0])
    }

    fn quantize_row(
        &mut self,
        luma: &[u8],
        output: &mut [u8],
        levels: &[f32],
    ) -> Result<(), RenderError> {
        if luma.len() != self.width {
            return Err(RenderError::InvalidDimensions);
        }
        require(output, self.width)?;
        self.next.fill(0.0);
        for x in 0..self.width {
            let old = (f32::from(luma[x]) + self.current[x + 1]).clamp(0.0, 255.0);
            let level = nearest_level(old, levels);
            output[x] = level as u8;
            let error = old - levels[level];
            self.current[x + 2] += error * (7.0 / 16.0);
            self.next[x] += error * (3.0 / 16.0);
            self.next[x + 1] += error * (5.0 / 16.0);
            self.next[x + 2] += error * (1.0 / 16.0);
        }
        core::mem::swap(&mut self.current, &mut self.next);
        Ok(())
    }
}

fn nearest_level(value: f32, levels: &[f32]) -> usize {
    let mut best = 0;
    let mut distance = (value - levels[0]).abs();
    for (index, level) in levels.iter().copied().enumerate().skip(1) {
        let candidate = (value - level).abs();
        if candidate < distance {
            best = index;
            distance = candidate;
        }
    }
    best
}

fn require(buffer: &[impl Sized], required: usize) -> Result<(), RenderError> {
    if buffer.len() < required {
        Err(RenderError::BufferTooSmall {
            required,
            provided: buffer.len(),
        })
    } else {
        Ok(())
    }
}
