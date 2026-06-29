#![no_std]

mod canonical;
mod dither;
mod error;
mod row;
mod staged;

pub use canonical::{canonical_bits, unpack, CanonicalGray2, PlaneBits};
pub use dither::ordered_bw;
pub use error::RenderError;
pub use row::{canonical_row_to_absolute, canonical_row_to_staged};
pub use staged::{staged_byte_to_absolute, staged_row_to_absolute, AbsolutePlaneByte};
