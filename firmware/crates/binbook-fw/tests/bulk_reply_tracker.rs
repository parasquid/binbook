#![cfg(feature = "diagnostic-console")]

use binbook_diagnostic_protocol::{FrameHeader, FrameKind, Opcode, Status};
use binbook_fw::bulk_reply_tracker::{BulkReplyTracker, PendingBulkReply};

fn reply(sequence: u32, opcode: Opcode) -> PendingBulkReply {
    PendingBulkReply {
        owner_sequence: sequence,
        header: FrameHeader {
            kind: FrameKind::Request,
            opcode,
            status: Status::Ok,
            sequence: sequence as u16,
            payload_len: 0,
        },
        stream_id: None,
    }
}

#[test]
fn bounded_tracker_rejects_overflow_without_losing_existing_reply() {
    let mut tracker = BulkReplyTracker::<1>::new();
    tracker.insert(reply(7, Opcode::StoreBulkOpen)).unwrap();
    assert!(tracker.is_full());
    assert!(tracker.insert(reply(8, Opcode::StoreBulkAbort)).is_err());
    assert_eq!(tracker.take(7).unwrap().owner_sequence, 7);
    assert!(tracker.take(8).is_none());
}

#[test]
fn finish_classification_and_sequence_take_are_exact() {
    let mut tracker = BulkReplyTracker::<2>::new();
    tracker.insert(reply(4, Opcode::StoreBulkOpen)).unwrap();
    tracker.insert(reply(5, Opcode::StoreBulkFinish)).unwrap();
    assert!(!tracker.is_finish(4));
    assert!(tracker.is_finish(5));
    assert_eq!(
        tracker.take(5).unwrap().header.opcode,
        Opcode::StoreBulkFinish
    );
    assert!(!tracker.is_finish(5));
    assert_eq!(tracker.take(4).unwrap().owner_sequence, 4);
}
