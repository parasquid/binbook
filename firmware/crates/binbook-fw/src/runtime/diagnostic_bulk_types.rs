use binbook_diagnostic_protocol::FrameHeader;

#[derive(Clone, Copy)]
pub(super) enum DeferredControl {
    Status { header: FrameHeader, stream_id: u32 },
    Finish { header: FrameHeader, stream_id: u32 },
}
