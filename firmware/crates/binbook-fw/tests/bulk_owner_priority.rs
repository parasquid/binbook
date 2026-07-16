#![cfg(feature = "diagnostic-console")]

use binbook_fw::storage_owner::{
    BufferSlot, OwnedMailbox, OwnedRequest, Request, RequestEnvelope, RequestPriority,
    StorageClient,
};

fn owned(sequence: u32, request: Request) -> OwnedRequest<'static> {
    OwnedRequest {
        envelope: RequestEnvelope {
            client: StorageClient::Diagnostic,
            sequence,
            request,
        },
        buffer: None,
    }
}

#[test]
fn abort_control_overtakes_queued_bulk_writes_for_the_same_owner_client() {
    let mut mailbox = OwnedMailbox::<4>::new();
    for (sequence, offset) in [(1, 0), (2, 512)] {
        mailbox
            .push(owned(
                sequence,
                Request::UploadWrite {
                    upload_id: 7,
                    offset,
                    buffer_offset: 0,
                    len: 512,
                    buffer: BufferSlot::bulk((sequence - 1) as u8).unwrap(),
                    power_save: false,
                },
            ))
            .unwrap();
    }
    mailbox
        .push(owned(3, Request::UploadAbort { upload_id: 7 }))
        .unwrap();

    let first = mailbox.pop().unwrap();
    assert_eq!(first.envelope.sequence, 3);
    assert_eq!(first.envelope.request.priority(), RequestPriority::Control);
    assert!(matches!(
        first.envelope.request,
        Request::UploadAbort { upload_id: 7 }
    ));
}
