//! Strategy engine: consume feed events and optional fill feedback, run strategies, emit signals.
//!
//! Holds a list of strategies and receivers for feed events and (optionally) fills. For each feed
//! event, calls each strategy's [Strategy::on_feed_event]. When a [StrategyFill] arrives, routes
//! it only to the strategy whose [Strategy::name] matches [StrategyFill::strategy_id]. Strategies
//! send [Signal]s to a channel consumed by the risk/execution layer.

use hft_core::events::EventEnvelope;
use hft_feed_handler::FeedEvent;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::debug;

use crate::strategies::{OrderAck, Signal, Strategy, StrategyFill};

/// Helper: receive from an optional UnboundedReceiver; pending forever if None.
async fn recv_opt<T>(rx: &mut Option<mpsc::UnboundedReceiver<T>>) -> Option<T> {
    match rx.as_mut() {
        Some(r) => r.recv().await,
        None => std::future::pending().await,
    }
}

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

    /// Run the engine: receive feed events, fills, and acks; dispatch to strategies.
    /// If `fill_rx` is provided, fills are routed to the strategy matching `fill.strategy_id`.
    /// If `ack_rx` is provided, acks are routed to the strategy matching `ack.strategy_id`.
    pub async fn run(
        &self,
        mut feed_rx: broadcast::Receiver<EventEnvelope<FeedEvent>>,
        mut fill_rx: Option<mpsc::UnboundedReceiver<StrategyFill>>,
        mut ack_rx: Option<mpsc::UnboundedReceiver<OrderAck>>,
    ) {
        loop {
            tokio::select! {
                res = feed_rx.recv() => match res {
                    Ok(envelope) => {
                        for s in &self.strategies {
                            s.on_feed_event(&envelope.payload, &self.signal_tx);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(lagged = n, "strategy engine lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                fill = recv_opt(&mut fill_rx) => match fill {
                    Some(f) => {
                        for s in &self.strategies {
                            if s.name() == f.strategy_id {
                                s.on_fill(&f);
                                break;
                            }
                        }
                    }
                    None => break,
                },
                ack = recv_opt(&mut ack_rx) => {
                    if let Some(a) = ack {
                        for s in &self.strategies {
                            if s.name() == a.strategy_id {
                                s.on_order_ack(&a);
                                break;
                            }
                        }
                    }
                    // ack_rx closing is non-fatal: continue running
                },
            }
        }
    }
}
