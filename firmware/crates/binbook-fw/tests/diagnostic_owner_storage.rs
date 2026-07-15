#![cfg(all(feature = "diagnostic-console", feature = "sd-storage"))]

use binbook_diagnostic_protocol::{
    encode_store_abort_request, encode_store_delete_request, encode_store_list_request,
    encode_store_read_request, encode_store_upload_begin_request,
    encode_store_upload_commit_request, encode_store_upload_write_request, Opcode, StorageBackend,
    StoreDeleteRequest, StoreListRequest, StoreReadRequest, StoreUploadBeginRequest,
    StoreUploadWriteRequest,
};
use binbook_fw::diag_owner_storage::{
    decode_owner_storage_command, DiagnosticUpload, OwnerStorageCommand, OwnerStorageDecodeError,
};

#[test]
fn owner_storage_decoder_preserves_list_read_and_delete_requests() {
    let mut payload = [0u8; 128];
    let mut transfer = [0u8; 4096];

    let len = encode_store_list_request(
        &StoreListRequest {
            backend: StorageBackend::Sd,
            start_index: 17,
        },
        &mut payload,
    )
    .unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreList, &payload[..len], &mut transfer).unwrap(),
        OwnerStorageCommand::List { start_index: 17 }
    );

    let len = encode_store_read_request(
        &StoreReadRequest {
            backend: StorageBackend::Sd,
            name: "a.binbook",
            offset: 123,
            max_bytes: 3000,
        },
        &mut payload,
    )
    .unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreRead, &payload[..len], &mut transfer).unwrap(),
        OwnerStorageCommand::Read {
            name: binbook_fw::storage_owner::StorageName::new("a.binbook").unwrap(),
            offset: 123,
            max_bytes: 3000,
        }
    );

    let len = encode_store_delete_request(
        &StoreDeleteRequest {
            backend: StorageBackend::Sd,
            name: "bad.binbook",
        },
        &mut payload,
    )
    .unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreDelete, &payload[..len], &mut transfer).unwrap(),
        OwnerStorageCommand::Delete {
            name: binbook_fw::storage_owner::StorageName::new("bad.binbook").unwrap(),
        }
    );

    payload[0] = StorageBackend::Flash as u8;
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreDelete, &payload[..len], &mut transfer),
        Err(OwnerStorageDecodeError::UnsupportedBackend)
    );
}

#[test]
fn owner_storage_decoder_copies_large_upload_data_and_preserves_ids() {
    let data = vec![0x5A; 2048];
    let mut payload = vec![0u8; 4096];
    let mut transfer = [0u8; 4096];
    let len = encode_store_upload_write_request(
        &StoreUploadWriteRequest {
            upload_id: 77,
            offset: 1024,
            data: &data,
        },
        &mut payload,
    )
    .unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreUploadWrite, &payload[..len], &mut transfer)
            .unwrap(),
        OwnerStorageCommand::UploadWrite {
            upload_id: 77,
            offset: 1024,
            len: 2048,
        }
    );
    assert_eq!(&transfer[..2048], data.as_slice());

    let len = encode_store_upload_commit_request(77, &mut payload).unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreUploadCommit, &payload[..len], &mut transfer)
            .unwrap(),
        OwnerStorageCommand::UploadCommit { upload_id: 77 }
    );
    let len = encode_store_abort_request(77, &mut payload).unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreAbort, &payload[..len], &mut transfer).unwrap(),
        OwnerStorageCommand::UploadAbort { upload_id: 77 }
    );
}

#[test]
fn diagnostic_upload_tracks_exact_size_offset_and_crc() {
    let bytes = b"owner-backed diagnostic upload larger than a tiny fixture";
    let expected_crc = binbook_fw::crc32::iso_hdlc(bytes);
    let mut upload = DiagnosticUpload::new(9, bytes.len() as u32, expected_crc);
    assert!(upload.accept(9, 0, &bytes[..17]).is_ok());
    assert!(upload.accept(9, 17, &bytes[17..]).is_ok());
    assert_eq!(upload.finish(9).unwrap(), (true, expected_crc));

    let mut wrong = DiagnosticUpload::new(10, 4, expected_crc);
    assert!(wrong.accept(11, 0, b"nope").is_err());
    assert!(wrong.accept(10, 1, b"nope").is_err());
    wrong.accept(10, 0, b"nope").unwrap();
    assert!(!wrong.finish(10).unwrap().0);
}

#[test]
fn owner_storage_decoder_preserves_upload_begin_crc_and_size() {
    let mut payload = [0u8; 128];
    let mut transfer = [0u8; 4096];
    let len = encode_store_upload_begin_request(
        &StoreUploadBeginRequest {
            backend: StorageBackend::Sd,
            name: "new.binbook",
            file_size: 0x0102_0304,
            expected_crc32: 0xA1B2_C3D4,
        },
        &mut payload,
    )
    .unwrap();
    assert_eq!(
        decode_owner_storage_command(Opcode::StoreUploadBegin, &payload[..len], &mut transfer)
            .unwrap(),
        OwnerStorageCommand::UploadBegin {
            name: binbook_fw::storage_owner::StorageName::new("new.binbook").unwrap(),
            file_size: 0x0102_0304,
            expected_crc32: 0xA1B2_C3D4,
        }
    );
}
