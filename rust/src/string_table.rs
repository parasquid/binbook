use crate::Error;

pub(crate) fn get_string<'a>(
    data: &'a [u8],
    string_table_offset: u64,
    string_table_length: u32,
    str_offset: u32,
    str_len: u32,
) -> Result<&'a [u8], Error> {
    if str_len == 0 {
        return Ok(b"");
    }
    if (str_offset as u64) + (str_len as u64) > string_table_length as u64 {
        return Err(Error::InvalidStringRef);
    }
    let abs = string_table_offset as usize + str_offset as usize;
    let end = abs + str_len as usize;
    if end > data.len() {
        return Err(Error::InvalidStringRef);
    }
    Ok(&data[abs..end])
}
