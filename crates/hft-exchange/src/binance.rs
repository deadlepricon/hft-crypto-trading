//! Binance exchange connector.
//!
//! Connects to Binance WebSocket API for depth and trades, and REST for orders.
//! This is a starter skeleton; full implementation would parse Binance-specific
//! JSON and map to [ExchangeMessage].

use async_trait::async_trait;
use hft_core::{OrderRequest, OrderBookSnapshot, HftError, Result};
use tokio::sync::mpsc;

use super::connector::{ExchangeConnector, ExchangeMessage};

/// Binance spot connector (WebSocket + REST).
/// Config holds URL and symbol; in production add API keys for order submission.
pub struct BinanceConnector {
    name: String,
    ws_base: String,
    symbol: String,
}

impl BinanceConnector {
    /// Create a Binance connector for the given symbol (e.g. "btcusdt").
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            name: "binance".to_string(),
            ws_base: "wss://stream.binance.com:9443/ws".to_string(),
            symbol: symbol.into(),
        }
    }

    /// Full WebSocket URL for depth stream.
    fn depth_stream_url(&self) -> String {
        format!("{}/{}@depth@100ms", self.ws_base, self.symbol)
    }

    /// Full WebSocket URL for agg trades.
    fn trades_stream_url(&self) -> String {
        format!("{}/{}@aggTrade", self.ws_base, self.symbol)
    }
}

#[async_trait]
impl ExchangeConnector for BinanceConnector {
    fn name(&self) -> &str {
        &self.name
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ExchangeMessage>> {
        let (tx, rx) = mpsc::unbounded_channel();
        // Starter: send Connected. Full impl would spawn tasks to connect to
        // depth_stream_url() and trades_stream_url(), parse JSON, and send
        // OrderBookDelta / Trade messages on tx.
        let _ = tx.send(ExchangeMessage::Connected);
        Ok(rx)
    }

    async fn submit_order(&self, _request: OrderRequest) -> Result<String> {
        // Placeholder: would POST to Binance REST API and return order id.
        Err(HftError::Exchange(
            "Binance submit_order not implemented (stub)".to_string(),
        ))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<()> {
        Err(HftError::Exchange(
            "Binance cancel_order not implemented (stub)".to_string(),
        ))
    }

    async fn fetch_order_book_snapshot(&self, _symbol: &str, _depth: u32) -> Result<Option<OrderBookSnapshot>> {
        Ok(None)
    }
}
