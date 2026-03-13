//! Execution engine: order lifecycle and fill handling.
//!
//! Receives [OrderRequest] from risk manager, calls exchange connector to
//! submit/cancel, and broadcasts [OrderEvent] / [FillEvent] for positions and UI.

use hft_core::OrderRequest;
use hft_exchange::ExchangeConnector;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Execution engine: submits orders and tracks their state.
pub struct ExecutionEngine {
    connector: Arc<dyn ExchangeConnector>,
    /// Channel to receive approved orders from risk manager.
    order_rx: mpsc::Receiver<OrderRequest>,
    /// Optional: notify risk manager of position changes (symbol, delta).
    risk_position_tx: Option<mpsc::Sender<(String, i64)>>,
}

impl ExecutionEngine {
    /// Create an engine that receives orders on the given channel.
    pub fn new(
        connector: Arc<dyn ExchangeConnector>,
        order_rx: mpsc::Receiver<OrderRequest>,
    ) -> Self {
        Self {
            connector,
            order_rx,
            risk_position_tx: None,
        }
    }

    /// Set a channel to push position deltas to the risk manager.
    pub fn set_risk_position_tx(&mut self, tx: mpsc::Sender<(String, i64)>) {
        self.risk_position_tx = Some(tx);
    }

    /// Run the engine: receive orders, submit to exchange, handle responses.
    pub async fn run(&mut self) -> hft_core::Result<()> {
        while let Some(req) = self.order_rx.recv().await {
            match self.connector.submit_order(req.clone()).await {
                Ok(order_id) => {
                    info!(order_id = %order_id, symbol = %req.symbol, "order submitted");
                    // In full impl: store pending order, subscribe to fill stream,
                    // on fill send FillEvent and position delta to risk.
                }
                Err(e) => {
                    warn!(error = %e, symbol = %req.symbol, "order submit failed");
                }
            }
        }
        Ok(())
    }
}
