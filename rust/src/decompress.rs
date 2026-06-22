use crate::Error;
use crate::page_index::{COMPRESSION_NONE, COMPRESSION_RLE_PACKBITS, COMPRESSION_LZ4};
use crate::PageRef;

pub fn decompress_page(page: &PageRef<'_>, out: &mut [u8]) -> Result<(), Error> {
    if out.len() < page.uncompressed_size {
        return Err(Error::OutputBufferTooSmall);
    }
    decompress_bytes(page.info.compression_method, page.compressed_data, out, page.uncompressed_size)
}

pub fn decompress_bytes(
    compression_method: u16,
    input: &[u8],
    out: &mut [u8],
    expected_size: usize,
) -> Result<(), Error> {
    if out.len() < expected_size {
        return Err(Error::OutputBufferTooSmall);
    }
    match compression_method {
        COMPRESSION_NONE => {
            if input.len() > out.len() {
                return Err(Error::OutputBufferTooSmall);
            }
            out[..input.len()].copy_from_slice(input);
            Ok(())
        }
        COMPRESSION_RLE_PACKBITS => {
            super::rle::decompress_packbits(input, out, expected_size)
        }
        #[cfg(feature = "lz4")]
        COMPRESSION_LZ4 => {
            super::lz4_decompress::decompress_lz4(input, out, expected_size)
        }
        #[cfg(not(feature = "lz4"))]
        COMPRESSION_LZ4 => {
            Err(Error::UnsupportedCompression(compression_method))
        }
        other => Err(Error::UnsupportedCompression(other)),
    }
}
