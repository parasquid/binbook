use crate::DecodeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackBitsRun {
    Literal { remaining: u8 },
    RepeatValue { remaining: u8 },
    Repeat { value: u8, remaining: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeProgress {
    pub consumed: usize,
    pub produced: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PackBitsDecoder {
    run: Option<PackBitsRun>,
}

impl PackBitsDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self { run: None }
    }

    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.run.is_none()
    }

    pub fn decode(
        &mut self,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<DecodeProgress, DecodeError> {
        let mut consumed = 0_usize;
        let mut produced = 0_usize;
        while produced < output.len() {
            match self.run {
                None => {
                    let Some(&control) = input.get(consumed) else {
                        break;
                    };
                    consumed += 1;
                    self.run = if control <= 127 {
                        Some(PackBitsRun::Literal {
                            remaining: control + 1,
                        })
                    } else {
                        Some(PackBitsRun::RepeatValue {
                            remaining: (control & 0x7f) + 1,
                        })
                    };
                }
                Some(PackBitsRun::Literal { remaining }) => {
                    let available_input = input.len() - consumed;
                    if available_input == 0 {
                        break;
                    }
                    let count = usize::from(remaining)
                        .min(available_input)
                        .min(output.len() - produced);
                    output[produced..produced + count]
                        .copy_from_slice(&input[consumed..consumed + count]);
                    consumed += count;
                    produced += count;
                    self.run = remaining_run(remaining, count, |remaining| PackBitsRun::Literal {
                        remaining,
                    });
                }
                Some(PackBitsRun::RepeatValue { remaining }) => {
                    let Some(&value) = input.get(consumed) else {
                        break;
                    };
                    consumed += 1;
                    self.run = Some(PackBitsRun::Repeat { value, remaining });
                }
                Some(PackBitsRun::Repeat { value, remaining }) => {
                    let count = usize::from(remaining).min(output.len() - produced);
                    output[produced..produced + count].fill(value);
                    produced += count;
                    self.run = remaining_run(remaining, count, |remaining| PackBitsRun::Repeat {
                        value,
                        remaining,
                    });
                }
            }
        }
        Ok(DecodeProgress { consumed, produced })
    }

    pub const fn finish(self) -> Result<(), DecodeError> {
        if self.run.is_none() {
            Ok(())
        } else {
            Err(DecodeError::MalformedRun)
        }
    }
}

pub(crate) fn decode_exact(input: &[u8], output: &mut [u8]) -> Result<(), DecodeError> {
    let mut decoder = PackBitsDecoder::new();
    let mut consumed = 0_usize;
    let mut produced = 0_usize;
    while produced < output.len() {
        let progress = decoder.decode(&input[consumed..], &mut output[produced..])?;
        consumed += progress.consumed;
        produced += progress.produced;
        if progress.consumed == 0 && progress.produced == 0 {
            break;
        }
    }
    if produced < output.len() {
        decoder.finish()?;
        return Err(DecodeError::OutputTooShort {
            expected: output.len(),
            actual: produced,
        });
    }
    if !decoder.is_idle() {
        return match decoder.run {
            Some(PackBitsRun::RepeatValue { .. }) if consumed == input.len() => {
                Err(DecodeError::MalformedRun)
            }
            Some(PackBitsRun::Literal { .. }) if consumed == input.len() => {
                Err(DecodeError::MalformedRun)
            }
            Some(PackBitsRun::Literal { .. } | PackBitsRun::Repeat { .. }) => {
                Err(DecodeError::OutputTooLong {
                    expected: output.len(),
                })
            }
            Some(PackBitsRun::RepeatValue { .. }) | None => Err(DecodeError::MalformedRun),
        };
    }
    if consumed < input.len() {
        let mut probe = [0_u8; 1];
        let progress = decoder.decode(&input[consumed..], &mut probe)?;
        if progress.produced > 0 {
            return Err(DecodeError::OutputTooLong {
                expected: output.len(),
            });
        }
        decoder.finish()?;
    }
    Ok(())
}

fn remaining_run(
    remaining: u8,
    consumed: usize,
    build: impl FnOnce(u8) -> PackBitsRun,
) -> Option<PackBitsRun> {
    let consumed = u8::try_from(consumed).ok()?;
    remaining
        .checked_sub(consumed)
        .filter(|value| *value > 0)
        .map(build)
}
