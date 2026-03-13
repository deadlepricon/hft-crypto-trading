//! Strategy implementations.
//!
//! Each strategy implements [Strategy]: it receives market data events and
//! may emit [Signal] for the risk/execution layer.

use hft_core::OrderRequest;
use hft_feed_handler::FeedEvent;
use std::time::Instant;

/// A trading signal: request to place an order (to be validated by risk manager).
#[derive(Debug, Clone)]
pub struct Signal {
    pub request: OrderRequest,
    pub strategy_id: String,
    pub generated_at: Instant,
}

/// Trait for strategy modules. The engine forwards [FeedEvent] and the
/// strategy may produce [Signal]s via a callback or channel.
pub trait Strategy: Send + Sync {
    /// Strategy name for logging and attribution.
    fn name(&self) -> &str;

    /// Process a market data event. May emit signals via the given sender.
    fn on_feed_event(&self, event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>);
}
