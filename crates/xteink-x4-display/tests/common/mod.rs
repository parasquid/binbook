#![allow(dead_code)]

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use xteink_x4_display::{
    engine::DisplayBackend,
    events::{DisplayEvent, EventSink, OperationOutcome},
    probes::ProbeKind,
    DisplayError, DisplayResult,
};

#[derive(Default)]
pub struct Events(pub Vec<DisplayEvent>);

impl EventSink for Events {
    fn emit(&mut self, event: DisplayEvent) {
        self.0.push(event);
    }
}

pub struct Backend {
    pub now: u64,
    pub epoch: u32,
    pub epoch_after_gray: Option<u32>,
    pub operations: Vec<&'static str>,
    pub gray: DisplayResult<OperationOutcome>,
    pub sync: DisplayResult<OperationOutcome>,
    pub render_bw: DisplayResult<()>,
    pub recovery: DisplayResult<()>,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            now: 0,
            epoch: 0,
            epoch_after_gray: None,
            operations: Vec::new(),
            gray: Ok(OperationOutcome::Completed),
            sync: Ok(OperationOutcome::Completed),
            render_bw: Ok(()),
            recovery: Ok(()),
        }
    }
}

impl DisplayBackend for Backend {
    fn timestamp_ms(&self) -> Option<u64> {
        Some(self.now)
    }
    fn request_epoch(&self) -> u32 {
        self.epoch
    }
    async fn init_bw(&mut self) -> DisplayResult<()> {
        self.operations.push("init-bw");
        Ok(())
    }
    async fn render_bw(&mut self, _: u32, _: u32) -> DisplayResult<()> {
        self.operations.push("render-bw");
        self.render_bw
    }
    async fn render_grayscale(&mut self, _: u32, _: u32) -> DisplayResult<OperationOutcome> {
        self.operations.push("render-gray");
        if let Some(epoch) = self.epoch_after_gray {
            self.epoch = epoch;
        }
        self.gray
    }
    async fn sync_bw_base(&mut self, _: u32, _: u32) -> DisplayResult<OperationOutcome> {
        self.operations.push("sync-base");
        self.sync
    }
    async fn recover_bw(&mut self, _: u32) -> DisplayResult<()> {
        self.operations.push("recover");
        self.recovery
    }
    async fn run_probe(&mut self, _: ProbeKind, _: u32) -> DisplayResult<()> {
        self.operations.push("probe");
        Ok(())
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

pub const FAILURE: DisplayError = DisplayError::Controller;
