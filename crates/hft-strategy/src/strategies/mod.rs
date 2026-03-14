//! Strategy implementations.
//!
//! Each strategy implements [Strategy]: it receives market data events and
//! may emit [Signal] for the risk/execution layer. Strategies can implement
//! the single [on_feed_event] and branch on [FeedEvent], or override
//! [on_orderbook_update], [on_trade], and [on_ticker_update] for clarity.
//! Optional [on_fill] receives fill feedback from the execution layer.

mod imbalance;
mod market_maker;
mod stub;

pub use imbalance::{ImbalanceParams, ImbalanceStrategy};
pub use market_maker::{MarketMakerParams, MarketMakerStrategy};
use hft_core::{OrderRequest, OrderSide, Price, Qty};
use hft_feed_handler::FeedEvent;
use std::time::Instant;

/// High-level signal intent: BUY / SELL / HOLD.
/// Strategies can use this internally; HOLD means no [Signal] is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalIntent {
    Buy,
    Sell,
    Hold,
}

/// A trading signal: request to place an order (to be validated by risk manager).
#[derive(Debug, Clone)]
pub struct Signal {
    pub request: OrderRequest,
    pub strategy_id: String,
    pub generated_at: Instant,
}

/// Approved order plus strategy attribution; sent from risk to execution so fills can be routed back.
#[derive(Debug, Clone)]
pub struct OrderWithStrategy {
    pub request: OrderRequest,
    pub strategy_id: String,
}

/// Fill event for strategy feedback (execution layer sends this so strategies can track inventory).
#[derive(Debug, Clone)]
pub struct StrategyFill {
    pub strategy_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub filled_qty: Qty,
    pub fill_price: Price,
}

/// Trait for strategy modules. The engine forwards [FeedEvent] and the
/// strategy may produce [Signal]s via the given sender. Default implementations
/// delegate to [on_feed_event]; override the specific callbacks for clarity.
/// Optional [on_fill] receives fill feedback when execution reports a fill for this strategy.
pub trait Strategy: Send + Sync {
    /// Strategy name for logging and attribution (must match [Signal::strategy_id] for fill routing).
    fn name(&self) -> &str;

    /// Process any market data event. Default calls the specific callbacks based on variant.
    fn on_feed_event(&self, event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        match event {
            FeedEvent::OrderBookSnapshot(_) => self.on_orderbook_update(event, signal_tx),
            FeedEvent::OrderBookDelta(_) => self.on_orderbook_update(event, signal_tx),
            FeedEvent::Trade(_) => self.on_trade(event, signal_tx),
            FeedEvent::Ticker(_) => self.on_ticker_update(event, signal_tx),
        }
    }

    /// Called when execution reports a fill for this strategy (paper or future live fills).
    /// Default: no-op. Override to track inventory accurately.
    fn on_fill(&self, _fill: &StrategyFill) {}

    /// Called on order book snapshot or delta. Default: no-op.
    fn on_orderbook_update(&self, _event: &FeedEvent, _signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
    }

    /// Called on public trade. Default: no-op.
    fn on_trade(&self, _event: &FeedEvent, _signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
    }

    /// Called on ticker update. Default: no-op.
    fn on_ticker_update(&self, _event: &FeedEvent, _signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
    }
}

/// Build an [OrderRequest] from symbol, side, optional price and qty (for limit/market).
pub fn order_request(
    symbol: impl Into<String>,
    side: OrderSide,
    qty: hft_core::Qty,
    price: Option<hft_core::Price>,
) -> OrderRequest {
    use hft_core::OrderType;
    OrderRequest {
        symbol: symbol.into(),
        side,
        order_type: if price.is_some() { OrderType::Limit } else { OrderType::Market },
        qty,
        price,
        time_in_force: None,
    }
}
