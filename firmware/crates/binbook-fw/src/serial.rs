#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command<'a> {
    List,
    Upload { name: &'a str, size: u32 },
    Delete { name: &'a str },
    Info,
    Page,
    Unknown,
}

pub struct SerialState<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> SerialState<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Option<Command<'_>> {
        for &byte in bytes {
            if byte == b'\n' {
                let command = core::str::from_utf8(&self.buf[..self.len])
                    .map(parse_command)
                    .unwrap_or(Command::Unknown);
                self.len = 0;
                return Some(command);
            }

            if self.len == N {
                self.len = 0;
                return Some(Command::Unknown);
            }

            self.buf[self.len] = byte;
            self.len += 1;
        }

        None
    }
}

impl<const N: usize> Default for SerialState<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_command(line: &str) -> Command<'_> {
    let line = line.trim();

    match line {
        "LIST" => Command::List,
        "INFO" => Command::Info,
        "PAGE" => Command::Page,
        _ => {
            if let Some(rest) = line.strip_prefix("UPLOAD ") {
                parse_upload(rest)
            } else if let Some(name) = line.strip_prefix("DELETE ") {
                let name = name.trim();
                if name.is_empty() {
                    Command::Unknown
                } else {
                    Command::Delete { name }
                }
            } else {
                Command::Unknown
            }
        }
    }
}

fn parse_upload(rest: &str) -> Command<'_> {
    let mut parts = rest.split_whitespace();
    let Some(name) = parts.next() else {
        return Command::Unknown;
    };
    let Some(size) = parts.next().and_then(|part| part.parse::<u32>().ok()) else {
        return Command::Unknown;
    };

    if parts.next().is_some() {
        return Command::Unknown;
    }

    Command::Upload { name, size }
}
