//! Exchange connector trait and message types.
//!
//! All exchange adapters normalize their API into these types so the feed
//! handler and execution engine stay exchange-agnostic.

use async_trait::async_trait;
use hft_core::{
    OrderBookSnapshot, OrderRequest, Result, TradeEvent,
    events::{FillEvent, OrderBookDelta, OrderEvent},
};
use tokio::sync::mpsc;

/// Normalized message from an exchange (depth update, trade, or order/fill).
#[derive(Debug, Clone)]
pub enum ExchangeMessage {
    /// Full or incremental order book update.
    OrderBookSnapshot(OrderBookSnapshot),
    OrderBookDelta(OrderBookDelta),
    /// Public trade.
    Trade(TradeEvent),
    /// Order status update (from execution).
    OrderEvent(OrderEvent),
    /// Fill notification.
    Fill(FillEvent),
    /// Connection or stream state.
    Connected,
    Disconnected { reason: String },
    /// Debug line (e.g. trade parse failure) for UI/logs.
    Debug(String),
}

/// Trait implemented by each exchange (Binance, etc.).
/// The feed handler holds a connector and subscribes to its message stream;
/// the execution engine uses the same connector to send orders.
#[async_trait]
pub trait ExchangeConnector: Send + Sync {
    /// Exchange identifier (e.g. "binance").
    fn name(&self) -> &str;

    /// Subscribe to market data and order/fill updates. The connector sends
    /// [ExchangeMessage] on the returned receiver.
    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ExchangeMessage>>;

    /// Submit an order. Returns exchange order id or error.
    async fn submit_order(&self, request: OrderRequest) -> Result<String>;

    /// Cancel an order by exchange order id.
    async fn cancel_order(&self, symbol: &str, order_id: &str) -> Result<()>;

    /// Optional: request a one-off order book snapshot (REST). If not needed, return Ok(None).
    async fn fetch_order_book_snapshot(&self, symbol: &str, depth: u32) -> Result<Option<OrderBookSnapshot>>;
}
