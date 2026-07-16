use crate::{
    transfer_report::HostTransferMetrics,
    upload::{Clock, ProgressEvent, ProgressSink},
};

pub struct MeasuringProgress<'a, P, C> {
    inner: P,
    clock: &'a C,
    metrics: HostTransferMetrics,
    data_started_ms: Option<u64>,
    finish_started_ms: Option<u64>,
    library_started_ms: Option<u64>,
}

impl<'a, P, C> MeasuringProgress<'a, P, C> {
    pub fn new(inner: P, clock: &'a C) -> Self {
        Self {
            inner,
            clock,
            metrics: HostTransferMetrics::default(),
            data_started_ms: None,
            finish_started_ms: None,
            library_started_ms: None,
        }
    }

    pub fn finish(mut self, status_queries: u32, retries: u32) -> HostTransferMetrics
    where
        C: Clock,
    {
        let now = elapsed_ms(self.clock);
        self.complete_open_phase(now);
        self.metrics.end_to_end_ms = now;
        self.metrics.status_queries = status_queries;
        self.metrics.retries = retries;
        self.metrics
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }

    fn observe(&mut self, event: &ProgressEvent)
    where
        C: Clock,
    {
        let now = elapsed_ms(self.clock);
        match event {
            ProgressEvent::Preparing { .. } => {
                self.metrics.prepare_ms = now;
                self.data_started_ms = Some(now);
            }
            ProgressEvent::Verifying => {
                self.metrics.data_ms = elapsed_since(self.data_started_ms.take(), now);
                self.finish_started_ms = Some(now);
            }
            ProgressEvent::Publishing => {
                self.metrics.finish_ms = elapsed_since(self.finish_started_ms.take(), now);
            }
            ProgressEvent::UpdatingLibrary => self.library_started_ms = Some(now),
            ProgressEvent::Complete(summary) => {
                self.complete_open_phase(now);
                self.metrics.end_to_end_ms = now;
                self.metrics.bytes_per_second = summary.bytes_per_second;
            }
            ProgressEvent::Transfer { .. } => {}
        }
    }

    fn complete_open_phase(&mut self, now: u64) {
        if let Some(start) = self.library_started_ms.take() {
            self.metrics.library_ms = now.saturating_sub(start);
        } else if let Some(start) = self.finish_started_ms.take() {
            self.metrics.finish_ms = now.saturating_sub(start);
        }
    }
}

impl<P: ProgressSink, C: Clock> ProgressSink for MeasuringProgress<'_, P, C> {
    fn report(&mut self, event: ProgressEvent) {
        self.observe(&event);
        self.inner.report(event);
    }
}

fn elapsed_since(start: Option<u64>, now: u64) -> u64 {
    start.map_or(0, |value| now.saturating_sub(value))
}

fn elapsed_ms(clock: &impl Clock) -> u64 {
    clock.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, time::Duration};

    use super::*;
    use crate::upload::UploadSummary;

    struct FakeClock(Cell<u64>);

    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            Duration::from_millis(self.0.get())
        }
    }

    struct Sink;
    impl ProgressSink for Sink {
        fn report(&mut self, _event: ProgressEvent) {}
    }

    #[test]
    fn derives_nonoverlapping_phases_from_observed_progress_events() {
        let clock = FakeClock(Cell::new(5));
        let mut progress = MeasuringProgress::new(Sink, &clock);
        progress.report(ProgressEvent::Preparing { total: 100 });
        clock.0.set(25);
        progress.report(ProgressEvent::Verifying);
        clock.0.set(35);
        progress.report(ProgressEvent::Publishing);
        clock.0.set(40);
        progress.report(ProgressEvent::UpdatingLibrary);
        clock.0.set(55);
        progress.report(ProgressEvent::Complete(UploadSummary {
            bytes: 100,
            crc32: 1,
            elapsed_ms: 55,
            bytes_per_second: 2_000,
            library_visible: true,
        }));
        let metrics = progress.finish(2, 1);
        assert_eq!(metrics.prepare_ms, 5);
        assert_eq!(metrics.data_ms, 20);
        assert_eq!(metrics.finish_ms, 10);
        assert_eq!(metrics.library_ms, 15);
        assert_eq!(metrics.end_to_end_ms, 55);
        assert_eq!(metrics.bytes_per_second, 2_000);
        assert_eq!((metrics.status_queries, metrics.retries), (2, 1));
    }
}
