//! Domain types used across the trading system.
//!
//! All monetary and quantity types use decimal-friendly representations
//! (e.g. string or fixed-point) to avoid float rounding issues in order logic.

use serde::{Deserialize, Serialize};

/// Trading pair symbol (e.g. "BTCUSDT").
pub type Symbol = String;

/// Price representation. Using string for exchange API compatibility and precision.
pub type Price = rust_decimal::Decimal;

/// Quantity representation.
pub type Qty = rust_decimal::Decimal;

/// Order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
}

/// Order status in the execution lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Submitted,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

/// Single price level in the order book (price + quantity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub price: Price,
    pub qty: Qty,
}

/// Unique identifier for an order (exchange-specific or internal).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId(pub String);

/// Unique identifier for a trade/fill.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeId(pub String);

/// Request to place an order (from strategy/risk layer).
#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub qty: Qty,
    pub price: Option<Price>,
    pub time_in_force: Option<TimeInForce>,
}

/// Time-in-force for limit orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC, // Good till cancel
    IOC, // Immediate or cancel
    FOK, // Fill or kill
}

/// Position in a single symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: Symbol,
    pub side: OrderSide,
    pub qty: Qty,
    pub entry_price: Price,
    pub unrealized_pnl: Option<Price>,
}
