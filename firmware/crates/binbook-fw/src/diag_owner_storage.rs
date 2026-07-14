//! Decoding and transaction state for diagnostic SD commands executed by the
//! single mounted storage owner.

use binbook_diagnostic_protocol::{
    decode_store_abort_request, decode_store_delete_request, decode_store_list_request,
    decode_store_read_request, decode_store_upload_begin_request,
    decode_store_upload_commit_request, decode_store_upload_write_request, Opcode, StorageBackend,
};

use crate::storage_owner::StorageName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerStorageCommand {
    List {
        start_index: u16,
    },
    Read {
        name: StorageName,
        offset: u32,
        max_bytes: u32,
    },
    Delete {
        name: StorageName,
    },
    UploadBegin {
        name: StorageName,
        file_size: u32,
        expected_crc32: u32,
    },
    UploadWrite {
        upload_id: u32,
        offset: u32,
        len: u16,
    },
    UploadCommit {
        upload_id: u32,
    },
    UploadAbort {
        upload_id: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerStorageDecodeError {
    BadPayload,
    UnsupportedBackend,
    Name,
    TooLarge,
    UnsupportedOpcode,
}

fn require_sd(backend: StorageBackend) -> Result<(), OwnerStorageDecodeError> {
    if backend == StorageBackend::Sd {
        Ok(())
    } else {
        Err(OwnerStorageDecodeError::UnsupportedBackend)
    }
}

fn name(value: &str) -> Result<StorageName, OwnerStorageDecodeError> {
    StorageName::new(value).map_err(|_| OwnerStorageDecodeError::Name)
}

pub fn decode_owner_storage_command(
    opcode: Opcode,
    payload: &[u8],
    transfer: &mut [u8],
) -> Result<OwnerStorageCommand, OwnerStorageDecodeError> {
    match opcode {
        Opcode::StoreList => {
            let request = decode_store_list_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            require_sd(request.backend)?;
            Ok(OwnerStorageCommand::List {
                start_index: request.start_index,
            })
        }
        Opcode::StoreRead => {
            let request = decode_store_read_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            require_sd(request.backend)?;
            Ok(OwnerStorageCommand::Read {
                name: name(request.name)?,
                offset: request.offset,
                max_bytes: request.max_bytes,
            })
        }
        Opcode::StoreDelete => {
            let request = decode_store_delete_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            require_sd(request.backend)?;
            Ok(OwnerStorageCommand::Delete {
                name: name(request.name)?,
            })
        }
        Opcode::StoreUploadBegin => {
            let request = decode_store_upload_begin_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            require_sd(request.backend)?;
            Ok(OwnerStorageCommand::UploadBegin {
                name: name(request.name)?,
                file_size: request.file_size,
                expected_crc32: request.expected_crc32,
            })
        }
        Opcode::StoreUploadWrite => {
            let request = decode_store_upload_write_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            let len =
                u16::try_from(request.data.len()).map_err(|_| OwnerStorageDecodeError::TooLarge)?;
            if request.data.len() > transfer.len() {
                return Err(OwnerStorageDecodeError::TooLarge);
            }
            transfer[..request.data.len()].copy_from_slice(request.data);
            Ok(OwnerStorageCommand::UploadWrite {
                upload_id: request.upload_id,
                offset: request.offset,
                len,
            })
        }
        Opcode::StoreUploadCommit => {
            let upload_id = decode_store_upload_commit_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            Ok(OwnerStorageCommand::UploadCommit { upload_id })
        }
        Opcode::StoreAbort => {
            let upload_id = decode_store_abort_request(payload)
                .map_err(|_| OwnerStorageDecodeError::BadPayload)?;
            Ok(OwnerStorageCommand::UploadAbort { upload_id })
        }
        _ => Err(OwnerStorageDecodeError::UnsupportedOpcode),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticUploadError {
    WrongId,
    WrongOffset,
    TooLarge,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticUpload {
    upload_id: u32,
    expected_size: u32,
    expected_crc32: u32,
    written: u32,
    crc_state: u32,
}

impl DiagnosticUpload {
    #[must_use]
    pub const fn new(upload_id: u32, expected_size: u32, expected_crc32: u32) -> Self {
        Self {
            upload_id,
            expected_size,
            expected_crc32,
            written: 0,
            crc_state: u32::MAX,
        }
    }

    pub fn accept(
        &mut self,
        upload_id: u32,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), DiagnosticUploadError> {
        if upload_id != self.upload_id {
            return Err(DiagnosticUploadError::WrongId);
        }
        if offset != self.written {
            return Err(DiagnosticUploadError::WrongOffset);
        }
        let end = offset
            .checked_add(bytes.len() as u32)
            .ok_or(DiagnosticUploadError::TooLarge)?;
        if end > self.expected_size {
            return Err(DiagnosticUploadError::TooLarge);
        }
        for byte in bytes {
            self.crc_state = crate::crc32::update_byte(self.crc_state, *byte);
        }
        self.written = end;
        Ok(())
    }

    pub fn finish(self, upload_id: u32) -> Result<(bool, u32), DiagnosticUploadError> {
        if upload_id != self.upload_id {
            return Err(DiagnosticUploadError::WrongId);
        }
        if self.written != self.expected_size {
            return Err(DiagnosticUploadError::Incomplete);
        }
        let computed = !self.crc_state;
        Ok((computed == self.expected_crc32, computed))
    }
}
