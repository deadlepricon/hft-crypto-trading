//! Strategy engine: consume feed events, run strategies, emit signals.
//!
//! Holds a list of strategies and a receiver for feed events. For each event,
//! calls each strategy's [Strategy::on_feed_event]. Strategies send [Signal]s
//! to a channel consumed by the risk/execution layer.

use hft_core::events::EventEnvelope;
use hft_feed_handler::FeedEvent;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::debug;

use crate::strategies::{Signal, Strategy};

/// Engine that runs multiple strategies and forwards their signals.
pub struct StrategyEngine {
    strategies: Vec<Arc<dyn Strategy>>,
    signal_tx: mpsc::Sender<Signal>,
}

impl StrategyEngine {
    /// Create an engine that will send signals to the given channel.
    pub fn new(signal_tx: mpsc::Sender<Signal>) -> Self {
        Self {
            strategies: Vec::new(),
            signal_tx,
        }
    }

    /// Register a strategy to be run on each feed event.
    pub fn add_strategy(&mut self, strategy: Arc<dyn Strategy>) {
        self.strategies.push(strategy);
    }

    /// Run the engine: receive feed events and dispatch to strategies.
    pub async fn run(
        &self,
        mut feed_rx: broadcast::Receiver<EventEnvelope<FeedEvent>>,
    ) {
        loop {
            match feed_rx.recv().await {
                Ok(envelope) => {
                    for s in &self.strategies {
                        s.on_feed_event(&envelope.payload, &self.signal_tx);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!(lagged = n, "strategy engine lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}
