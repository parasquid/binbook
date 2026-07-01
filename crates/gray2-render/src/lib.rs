#![no_std]

mod canonical;
mod dither;
mod error;
mod image;
mod pack;
mod quantize;
mod row;
mod staged;

pub use canonical::{canonical_bits, unpack, CanonicalGray2, PlaneBits};
pub use dither::ordered_bw;
pub use error::RenderError;
pub use image::{canonical_image_to_staged, PlaneChunks};
pub use pack::{pack_gray1_row, pack_gray2_row};
pub use quantize::{quantize_gray1, quantize_gray2, FloydSteinberg};
pub use row::{canonical_row_to_absolute, canonical_row_to_staged};
pub use staged::{staged_byte_to_absolute, staged_row_to_absolute, AbsolutePlaneByte};
