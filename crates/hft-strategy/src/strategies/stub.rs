//! Stub strategy: no real logic, placeholder for wiring.

use super::{Signal, Strategy};
use hft_feed_handler::FeedEvent;

/// Strategy that does not send any orders; used to validate the pipeline.
#[derive(Default)]
pub struct StubStrategy;

impl Strategy for StubStrategy {
    fn name(&self) -> &str {
        "stub"
    }

    fn on_feed_event(&self, _event: &FeedEvent, _signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        // No-op: no signals emitted.
    }
}
