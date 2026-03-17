//! Event types broadcast between components.
//!
//! The feed handler produces market data events; the order book produces
//! snapshots/deltas; the execution engine produces fill and order status events.
//! Strategies and the UI consume these events.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{Level, OrderId, OrderSide, OrderStatus, Price, Qty, Symbol, TradeId};

/// High-level event envelope; component source and timestamp.
#[derive(Debug, Clone)]
pub struct EventEnvelope<T> {
    pub source: EventSource,
    pub ts: DateTime<Utc>,
    pub payload: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSource {
    FeedHandler,
    OrderBook,
    Strategy,
    Risk,
    Execution,
    Exchange,
}

/// Order book snapshot (full state at a point in time).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
    pub sequence: u64,
}

/// Order book delta (incremental update).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookDelta {
    pub symbol: Symbol,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
    pub sequence: u64,
}

/// A public trade from the exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    pub symbol: Symbol,
    pub trade_id: TradeId,
    pub price: Price,
    pub qty: Qty,
    pub side: OrderSide,
    pub timestamp: DateTime<Utc>,
}

/// Order lifecycle event (submit, fill, cancel, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEvent {
    pub order_id: OrderId,
    pub symbol: Symbol,
    pub status: OrderStatus,
    pub filled_qty: Qty,
    pub avg_fill_price: Option<Price>,
    pub message: Option<String>,
}

/// Fill (execution) event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillEvent {
    pub order_id: OrderId,
    pub trade_id: TradeId,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub price: Price,
    pub qty: Qty,
    pub timestamp: DateTime<Utc>,
    /// Client order id from the order (e.g. strategy_id), when exchange echoes it back.
    pub client_order_id: Option<String>,
}
