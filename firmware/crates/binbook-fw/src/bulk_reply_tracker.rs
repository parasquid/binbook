use binbook_diagnostic_protocol::{FrameHeader, Opcode};
use heapless::Vec;

#[derive(Debug, Clone, Copy)]
pub struct PendingBulkReply {
    pub owner_sequence: u32,
    pub header: FrameHeader,
    pub stream_id: Option<u32>,
}

pub struct BulkReplyTracker<const N: usize> {
    replies: Vec<PendingBulkReply, N>,
}

impl<const N: usize> BulkReplyTracker<N> {
    pub const fn new() -> Self {
        Self {
            replies: Vec::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.replies.is_full()
    }

    pub fn insert(&mut self, reply: PendingBulkReply) -> Result<(), PendingBulkReply> {
        self.replies.push(reply)
    }

    pub fn take(&mut self, sequence: u32) -> Option<PendingBulkReply> {
        let index = self
            .replies
            .iter()
            .position(|reply| reply.owner_sequence == sequence)?;
        Some(self.replies.remove(index))
    }

    pub fn is_finish(&self, sequence: u32) -> bool {
        self.replies.iter().any(|reply| {
            reply.owner_sequence == sequence && reply.header.opcode == Opcode::StoreBulkFinish
        })
    }
}

impl<const N: usize> Default for BulkReplyTracker<N> {
    fn default() -> Self {
        Self::new()
    }
}
