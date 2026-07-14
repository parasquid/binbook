#![cfg(feature = "sd-storage")]

use binbook_fw::{
    reader_state::{ProgressRecord, ReaderState, MAX_PROGRESS_RECORDS, MAX_STATE_BYTES},
    state_owner::{OwnerStateSlots, StateLoadUpdate, StateSaveUpdate},
    storage_owner::{CompletionError, Request, Response, ResponseEnvelope, StorageClient},
};

fn response(sequence: u32, value: Result<Response, CompletionError>) -> ResponseEnvelope {
    ResponseEnvelope {
        client: StorageClient::State,
        sequence,
        response: value,
    }
}

fn encoded_state(generation: u32, records: usize) -> (ReaderState, Vec<u8>) {
    let mut state = ReaderState::empty();
    state.generation = generation;
    for index in 0..records {
        let mut rendition_hash = [0x55; 32];
        rendition_hash[..8].copy_from_slice(&(index as u64).to_le_bytes());
        state
            .set_progress(ProgressRecord {
                filename_hash: index as u64 + 1,
                rendition_hash,
                page: index as u32,
            })
            .unwrap();
    }
    let mut bytes = vec![0; MAX_STATE_BYTES];
    let used = state.encode(&mut bytes).unwrap();
    bytes.truncate(used);
    (state, bytes)
}

#[test]
fn load_streams_both_slots_and_selects_newest_valid_generation() {
    let (older, slot_a) = encoded_state(7, MAX_PROGRESS_RECORDS);
    let (newer, slot_b) = encoded_state(8, MAX_PROGRESS_RECORDS - 1);
    let mut slots = OwnerStateSlots::new();
    let mut loaded = ReaderState::empty();
    let mut buffer = [0u8; MAX_STATE_BYTES];
    let (mut client, mut request) = slots.begin_load(&mut loaded, 10);
    loop {
        let update = match &request.request {
            Request::FileSize { name } => {
                let bytes = if name.as_str().ends_with("A.STATE") {
                    &slot_a
                } else {
                    &slot_b
                };
                client
                    .accept(
                        response(request.sequence, Ok(Response::FileSize(bytes.len() as u64))),
                        &mut buffer,
                    )
                    .unwrap()
            }
            Request::Read {
                name,
                offset,
                buffer_offset,
                len,
                ..
            } => {
                assert!(usize::from(*len) <= 4096);
                let source = if name.as_str().ends_with("A.STATE") {
                    &slot_a
                } else {
                    &slot_b
                };
                let start = *offset as usize;
                let target = usize::from(*buffer_offset);
                buffer[target..target + usize::from(*len)]
                    .copy_from_slice(&source[start..start + usize::from(*len)]);
                client
                    .accept(
                        response(request.sequence, Ok(Response::Read(*len))),
                        &mut buffer,
                    )
                    .unwrap()
            }
            _ => panic!("unexpected load request"),
        };
        match update {
            StateLoadUpdate::Request(next) => request = next,
            StateLoadUpdate::Complete { warning } => {
                assert!(!warning);
                break;
            }
        }
    }
    drop(client);
    assert_eq!(loaded, newer);
    assert_ne!(loaded, older);
}

#[test]
fn save_streams_maximum_state_and_commits_generation_only_after_readback() {
    let (mut state, _) = encoded_state(0, MAX_PROGRESS_RECORDS);
    let mut slots = OwnerStateSlots::new();
    let mut buffer = [0u8; MAX_STATE_BYTES];
    let (mut client, mut request) = slots.begin_save(&mut state, &mut buffer, 100).unwrap();
    let mut persisted = vec![0u8; MAX_STATE_BYTES];
    let mut used = 0usize;
    loop {
        let update = match request.request {
            Request::UploadBegin { expected_size, .. } => {
                used = expected_size as usize;
                client
                    .accept(
                        response(request.sequence, Ok(Response::UploadStarted(9))),
                        &mut buffer,
                    )
                    .unwrap()
            }
            Request::UploadWrite { offset, len, .. } => {
                assert!(usize::from(len) <= 512);
                persisted[offset as usize..offset as usize + usize::from(len)]
                    .copy_from_slice(&buffer[offset as usize..offset as usize + usize::from(len)]);
                client
                    .accept(
                        response(request.sequence, Ok(Response::Written(len))),
                        &mut buffer,
                    )
                    .unwrap()
            }
            Request::UploadCommit { .. } => client
                .accept(response(request.sequence, Ok(Response::Ack)), &mut buffer)
                .unwrap(),
            Request::Read {
                offset,
                buffer_offset,
                len,
                ..
            } => {
                let start = offset as usize;
                let target = usize::from(buffer_offset);
                buffer[target..target + usize::from(len)]
                    .copy_from_slice(&persisted[start..start + usize::from(len)]);
                client
                    .accept(
                        response(request.sequence, Ok(Response::Read(len))),
                        &mut buffer,
                    )
                    .unwrap()
            }
            _ => panic!("unexpected save request"),
        };
        match update {
            StateSaveUpdate::Request(next) => request = next,
            StateSaveUpdate::Complete => break,
        }
    }
    drop(client);
    assert_eq!(state.generation, 1);
    assert_eq!(ReaderState::decode(&persisted[..used]).unwrap(), state);
}

#[test]
fn later_write_failure_aborts_and_does_not_commit_ram_generation() {
    let (mut state, _) = encoded_state(0, 20);
    let mut slots = OwnerStateSlots::new();
    let mut buffer = [0u8; MAX_STATE_BYTES];
    let (mut client, begin) = slots.begin_save(&mut state, &mut buffer, 200).unwrap();
    let StateSaveUpdate::Request(first_write) = client
        .accept(
            response(begin.sequence, Ok(Response::UploadStarted(11))),
            &mut buffer,
        )
        .unwrap()
    else {
        panic!("first write required")
    };
    let Request::UploadWrite { len, .. } = first_write.request else {
        panic!("first write required")
    };
    let StateSaveUpdate::Request(second_write) = client
        .accept(
            response(first_write.sequence, Ok(Response::Written(len))),
            &mut buffer,
        )
        .unwrap()
    else {
        panic!("second write required")
    };
    let abort = client
        .accept(
            response(second_write.sequence, Err(CompletionError::Backend)),
            &mut buffer,
        )
        .unwrap();
    let StateSaveUpdate::Request(abort) = abort else {
        panic!("failed upload must be aborted")
    };
    assert!(matches!(
        abort.request,
        Request::UploadAbort { upload_id: 11 }
    ));
    assert!(client
        .accept(response(abort.sequence, Ok(Response::Ack)), &mut buffer)
        .is_err());
    drop(client);
    assert_eq!(state.generation, 0);
}
