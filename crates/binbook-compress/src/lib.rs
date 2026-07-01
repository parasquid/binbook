#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

mod packbits;

pub use packbits::{encode_into, encoded_len, EncodeError};

#[cfg(feature = "alloc")]
pub use packbits::encode;
