#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Spi,
    Gpio,
    Timeout,
    InvalidParameter,
}
