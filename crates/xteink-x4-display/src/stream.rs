use binbook_core::{CompressionMethod, ReadAt};
use binbook_decompress::PackBitsDecoder;

use crate::{buffers::RenderBuffers, DisplayError, DisplayResult};

pub fn decode_stream<R, F>(
    source: &mut R,
    offset: u64,
    compressed_len: usize,
    method: CompressionMethod,
    decoded_len: usize,
    buffers: &mut RenderBuffers<'_>,
    mut emit: F,
) -> DisplayResult<()>
where
    R: ReadAt,
    F: FnMut(&[u8]) -> DisplayResult<()>,
{
    buffers.require_streaming()?;
    match method {
        CompressionMethod::None => stream_none(
            source,
            offset,
            compressed_len,
            decoded_len,
            buffers.decoded,
            emit,
        ),
        CompressionMethod::RlePackBits => stream_packbits(
            source,
            offset,
            compressed_len,
            decoded_len,
            buffers.compressed,
            buffers.decoded,
            emit,
        ),
        CompressionMethod::Lz4 => {
            require(buffers.compressed.len(), compressed_len)?;
            require(buffers.decoded.len(), decoded_len)?;
            source
                .read_exact_at(offset, &mut buffers.compressed[..compressed_len])
                .map_err(|_| DisplayError::Source)?;
            binbook_decompress::decode_exact(
                method,
                &buffers.compressed[..compressed_len],
                &mut buffers.decoded[..decoded_len],
            )?;
            emit(&buffers.decoded[..decoded_len])
        }
    }
}

fn stream_none<R, F>(
    source: &mut R,
    offset: u64,
    compressed_len: usize,
    decoded_len: usize,
    decoded: &mut [u8],
    mut emit: F,
) -> DisplayResult<()>
where
    R: ReadAt,
    F: FnMut(&[u8]) -> DisplayResult<()>,
{
    if compressed_len != decoded_len {
        return Err(DisplayError::Decode);
    }
    let mut position = 0;
    while position < decoded_len {
        let count = decoded.len().min(decoded_len - position);
        source
            .read_exact_at(offset + position as u64, &mut decoded[..count])
            .map_err(|_| DisplayError::Source)?;
        emit(&decoded[..count])?;
        position += count;
    }
    Ok(())
}

fn stream_packbits<R, F>(
    source: &mut R,
    offset: u64,
    compressed_len: usize,
    decoded_len: usize,
    compressed: &mut [u8],
    decoded: &mut [u8],
    mut emit: F,
) -> DisplayResult<()>
where
    R: ReadAt,
    F: FnMut(&[u8]) -> DisplayResult<()>,
{
    let mut decoder = PackBitsDecoder::new();
    let mut source_position = 0;
    let mut input_position = 0;
    let mut input_length = 0;
    let mut output_position = 0;
    while output_position < decoded_len {
        let output_length = decoded.len().min(decoded_len - output_position);
        let mut written = 0;
        while written < output_length {
            let progress = decoder.decode(
                &compressed[input_position..input_length],
                &mut decoded[written..output_length],
            )?;
            input_position += progress.consumed;
            written += progress.produced;
            if progress.consumed == 0 && progress.produced == 0 {
                if input_position != input_length || source_position == compressed_len {
                    return Err(DisplayError::Decode);
                }
                input_length = compressed.len().min(compressed_len - source_position);
                source
                    .read_exact_at(
                        offset + source_position as u64,
                        &mut compressed[..input_length],
                    )
                    .map_err(|_| DisplayError::Source)?;
                source_position += input_length;
                input_position = 0;
            }
        }
        emit(&decoded[..output_length])?;
        output_position += output_length;
    }
    if source_position != compressed_len || input_position != input_length || !decoder.is_idle() {
        return Err(DisplayError::Decode);
    }
    decoder.finish()?;
    Ok(())
}

fn require(provided: usize, required: usize) -> DisplayResult<()> {
    if provided < required {
        Err(DisplayError::BufferTooSmall { required, provided })
    } else {
        Ok(())
    }
}
