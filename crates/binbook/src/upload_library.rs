use binbook_diagnostic_protocol::{
    decode_store_list_count, decode_store_list_entry, encode_store_list_request, Opcode, Status,
    StorageBackend, StoreListRequest, STORE_LIST_ENTRY_HEADER_BYTES,
};

use crate::{
    upload_protocol::{checked_payload, encode_request, Sequence},
    upload_types::{UploadError, UploadTransport},
};

pub(crate) fn wait_for_library<T: UploadTransport>(
    transport: &mut T,
    destination: &str,
    size: u64,
    sequence: &mut Sequence,
) -> Result<bool, UploadError> {
    'polls: for poll in 0..10 {
        let mut start_index = 0u16;
        loop {
            let list_sequence = sequence.take();
            let request = StoreListRequest {
                backend: StorageBackend::Sd,
                start_index,
            };
            let frame = encode_request(list_sequence, Opcode::StoreList, |payload| {
                encode_store_list_request(&request, payload)
            })?;
            let response = match transport.exchange(&frame, Opcode::StoreList, list_sequence) {
                Ok(response) => response,
                Err(error) if is_transient_library_scan_error(&error) => {
                    if poll != 9 {
                        transport.wait_library_poll();
                    }
                    continue 'polls;
                }
                Err(error) => return Err(UploadError::LibraryWait(error.to_string())),
            };
            let payload = match checked_payload(&response, Opcode::StoreList, list_sequence) {
                Ok(payload) => payload,
                Err(error) if is_transient_library_scan_error(&error) => {
                    if poll != 9 {
                        transport.wait_library_poll();
                    }
                    continue 'polls;
                }
                Err(error) => return Err(UploadError::LibraryWait(error.to_string())),
            };
            let count = decode_store_list_count(&payload)?;
            if count == 0 {
                break;
            }
            let mut position = 2usize;
            for _ in 0..count {
                let entry = decode_store_list_entry(&payload[position..])?;
                position += STORE_LIST_ENTRY_HEADER_BYTES + entry.name.len();
                if entry.name == destination && entry.file_size == size {
                    return Ok(true);
                }
            }
            start_index = start_index
                .checked_add(count)
                .ok_or_else(|| UploadError::LibraryWait("list cursor overflow".into()))?;
        }
        if poll != 9 {
            transport.wait_library_poll();
        }
    }
    Ok(false)
}

pub(crate) fn is_transient_library_scan_error(error: &UploadError) -> bool {
    matches!(
        error,
        UploadError::DeviceStatus {
            opcode: Opcode::StoreList,
            status: Status::Error,
            ..
        }
    )
}
