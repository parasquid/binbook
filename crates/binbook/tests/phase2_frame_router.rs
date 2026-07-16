use binbook::frame_router::{FrameRouter, RouterError};
use binbook_diagnostic_protocol::{
    encode_frame, FrameHeader, FrameKind, Opcode, Status, FRAME_DELIMITER, MAX_FRAME_BYTES,
};

fn frame(
    kind: FrameKind,
    opcode: Opcode,
    status: Status,
    sequence: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = [0; MAX_FRAME_BYTES];
    let len = encode_frame(
        &FrameHeader {
            kind,
            opcode,
            status,
            sequence,
            payload_len: payload.len() as u16,
        },
        payload,
        &mut out,
    )
    .unwrap();
    out[..len].to_vec()
}

#[test]
fn router_preserves_partial_and_unmatched_complete_frames() {
    let event = frame(
        FrameKind::Event,
        Opcode::StoreBulkCredit,
        Status::Ok,
        0,
        b"event",
    );
    let response = frame(
        FrameKind::Response,
        Opcode::StoreBulkOpen,
        Status::Ok,
        7,
        b"open",
    );
    let split = response.len() / 2;
    let mut router = FrameRouter::new();
    router.push(&event).unwrap();
    router.push(&response[..split]).unwrap();
    assert_eq!(router.pending_frames(), 1);
    assert!(router
        .take_response(Opcode::StoreBulkOpen, 7)
        .unwrap()
        .is_none());
    router.push(&response[split..]).unwrap();

    let routed = router
        .take_response(Opcode::StoreBulkOpen, 7)
        .unwrap()
        .unwrap();
    assert_eq!(routed.payload, b"open");
    assert_eq!(router.pending_frames(), 1);
    let event = router
        .take_bulk_event(Opcode::StoreBulkCredit)
        .unwrap()
        .unwrap();
    assert_eq!(event.payload, b"event");
    assert_eq!(router.pending_frames(), 0);
}

#[test]
fn router_rejects_duplicate_response_and_wrong_opcode_for_sequence() {
    let response = frame(
        FrameKind::Response,
        Opcode::StoreBulkOpen,
        Status::Ok,
        7,
        b"",
    );
    let mut router = FrameRouter::new();
    let mut duplicate = response.clone();
    duplicate.extend_from_slice(&response);
    router.push(&duplicate).unwrap();
    assert_eq!(
        router.take_response(Opcode::StoreBulkOpen, 7),
        Err(RouterError::DuplicateResponse(7))
    );

    let mut router = FrameRouter::new();
    router
        .push(&frame(
            FrameKind::Response,
            Opcode::StoreBulkStatus,
            Status::Ok,
            7,
            b"",
        ))
        .unwrap();
    assert_eq!(
        router.take_response(Opcode::StoreBulkOpen, 7),
        Err(RouterError::WrongOpcode {
            expected: Opcode::StoreBulkOpen,
            actual: Opcode::StoreBulkStatus,
            sequence: 7,
        })
    );
}

#[test]
fn router_enforces_bulk_event_kind_sequence_and_status() {
    for (kind, status, sequence) in [
        (FrameKind::Request, Status::Ok, 0),
        (FrameKind::Event, Status::Error, 0),
        (FrameKind::Event, Status::Ok, 1),
    ] {
        let mut router = FrameRouter::new();
        router
            .push(&frame(kind, Opcode::StoreBulkData, status, sequence, b""))
            .unwrap();
        assert!(router.take_bulk_event(Opcode::StoreBulkData).is_err());
    }
}

#[test]
fn router_rejects_malformed_and_bounds_queued_frames() {
    let mut router = FrameRouter::new();
    assert_eq!(
        router.push(&[1, 2, FRAME_DELIMITER]),
        Err(RouterError::MalformedFrame)
    );

    let event = frame(FrameKind::Event, Opcode::StoreBulkData, Status::Ok, 0, b"");
    for _ in 0..FrameRouter::MAX_QUEUED_FRAMES {
        router.push(&event).unwrap();
    }
    assert_eq!(router.push(&event), Err(RouterError::QueueFull));
}

#[test]
fn router_reports_complete_and_unterminated_oversize_distinctly() {
    let mut router = FrameRouter::new();
    let unterminated = vec![1; MAX_FRAME_BYTES + 1];
    assert_eq!(
        router.push(&unterminated),
        Err(RouterError::FrameTooLarge {
            actual: MAX_FRAME_BYTES + 1,
            max: MAX_FRAME_BYTES,
            complete: false,
        })
    );

    let mut router = FrameRouter::new();
    let mut complete = vec![1; MAX_FRAME_BYTES + 1];
    complete.push(FRAME_DELIMITER);
    assert_eq!(
        router.push(&complete),
        Err(RouterError::FrameTooLarge {
            actual: MAX_FRAME_BYTES + 2,
            max: MAX_FRAME_BYTES,
            complete: true,
        })
    );
}
