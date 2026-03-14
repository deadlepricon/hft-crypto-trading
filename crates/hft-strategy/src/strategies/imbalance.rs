//! Order book imbalance strategy: what we look for and when we execute.
//!
//! **What we look for**
//! - Top N levels of the order book (bids vs asks).
//! - Total bid quantity vs total ask quantity (imbalance).
//!
//! **When we execute**
//! - **BUY** when we *enter* bid-heavy (imbalance >= threshold_buy) — one signal per regime.
//! - **SELL** when we *enter* ask-heavy (imbalance <= -threshold_sell) — one signal per regime.
//! - You can set **asymmetric thresholds**: e.g. higher threshold_sell (only sell when clearly ask-heavy)
//!   and lower threshold_buy (buy back sooner when book tilts back) to improve expectancy.
//! - Optional **confirmation**: require regime to persist for 2 book updates before signalling (reduces whipsaw).

use hft_core::{OrderSide, Qty};
use hft_feed_handler::FeedEvent;
use hft_order_book::OrderBook;
use rust_decimal::Decimal;
use std::sync::{Arc, Mutex};
use tracing::debug;

use super::{order_request, Signal, Strategy};

/// Which imbalance regime we're in; used to signal only on *transition*.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Regime {
    BidHeavy,
    AskHeavy,
    Neutral,
}

/// Configurable parameters for the imbalance strategy.
#[derive(Debug, Clone)]
pub struct ImbalanceParams {
    /// Number of top levels on each side to sum for imbalance.
    pub book_depth: usize,
    /// Default threshold when buy/sell specific ones are not set (same units as book qty, e.g. BTC).
    pub imbalance_threshold: Decimal,
    /// Optional: threshold for BUY (bid-heavy). If None, uses imbalance_threshold. Lower = exit shorts sooner.
    pub imbalance_threshold_buy: Option<Decimal>,
    /// Optional: threshold for SELL (ask-heavy). If None, uses imbalance_threshold. Higher = only sell when clearly ask-heavy.
    pub imbalance_threshold_sell: Option<Decimal>,
    /// Fixed order size per signal (e.g. 0.001 BTC).
    pub order_size: Qty,
    /// If true, send limit order at best bid/ask; else market.
    pub use_limit: bool,
    /// Require regime to persist for this many book updates before signalling (0 = disabled, 2 = confirm once).
    pub confirm_ticks: u32,
}

impl Default for ImbalanceParams {
    fn default() -> Self {
        Self {
            book_depth: 10,
            imbalance_threshold: Decimal::new(1, 2), // 0.01
            imbalance_threshold_buy: None,
            imbalance_threshold_sell: None,
            order_size: Decimal::new(1, 3), // 0.001
            use_limit: true,
            confirm_ticks: 2, // require 2 consecutive ticks in same regime to reduce whipsaw
        }
    }
}

/// Strategy that trades in the direction of order book imbalance.
/// Signals only on regime transition (and after confirm_ticks when set).
pub struct ImbalanceStrategy {
    order_book: Arc<OrderBook>,
    symbol: String,
    params: ImbalanceParams,
    last_regime: Mutex<Regime>,
    regime_ticks: Mutex<u32>,
}

impl ImbalanceStrategy {
    pub fn new(order_book: Arc<OrderBook>, symbol: impl Into<String>, params: ImbalanceParams) -> Self {
        Self {
            order_book,
            symbol: symbol.into(),
            params,
            last_regime: Mutex::new(Regime::Neutral),
            regime_ticks: Mutex::new(0),
        }
    }
}

impl Strategy for ImbalanceStrategy {
    fn name(&self) -> &str {
        "imbalance"
    }

    fn on_orderbook_update(&self, _event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        let (bids, asks, _seq) = self.order_book.snapshot(self.params.book_depth);
        let bid_qty: Decimal = bids.iter().map(|l| l.qty).sum();
        let ask_qty: Decimal = asks.iter().map(|l| l.qty).sum();

        let imbalance = bid_qty - ask_qty;
        let th_buy = self.params.imbalance_threshold_buy.unwrap_or(self.params.imbalance_threshold);
        let th_sell = self.params.imbalance_threshold_sell.unwrap_or(self.params.imbalance_threshold);

        let regime = if imbalance >= th_buy {
            Regime::BidHeavy
        } else if imbalance <= -th_sell {
            Regime::AskHeavy
        } else {
            Regime::Neutral
        };

        let (should_buy, should_sell) = {
            let mut last = self.last_regime.lock().unwrap();
            let mut ticks = self.regime_ticks.lock().unwrap();

            let regime_changed = *last != regime;
            if regime_changed {
                *ticks = 1;
            } else {
                *ticks = ticks.saturating_add(1);
            }
            *last = regime;

            let ct = self.params.confirm_ticks;
            // Signal when we're in the regime and: either no confirmation, or this tick completes confirmation.
            let should_buy = regime == Regime::BidHeavy
                && ((regime_changed && ct == 0) || (!regime_changed && *ticks == ct));
            let should_sell = regime == Regime::AskHeavy
                && ((regime_changed && ct == 0) || (!regime_changed && *ticks == ct));

            (should_buy, should_sell)
        };

        if should_buy {
            let price = self.params.use_limit.then(|| asks.first().map(|l| l.price)).flatten();
            let req = order_request(
                self.symbol.clone(),
                OrderSide::Buy,
                self.params.order_size,
                price,
            );
            let _ = signal_tx.try_send(Signal {
                request: req,
                strategy_id: self.name().to_string(),
                generated_at: std::time::Instant::now(),
            });
            debug!(imbalance = %imbalance, "imbalance BUY (transition)");
        } else if should_sell {
            let price = self.params.use_limit.then(|| bids.first().map(|l| l.price)).flatten();
            let req = order_request(
                self.symbol.clone(),
                OrderSide::Sell,
                self.params.order_size,
                price,
            );
            let _ = signal_tx.try_send(Signal {
                request: req,
                strategy_id: self.name().to_string(),
                generated_at: std::time::Instant::now(),
            });
            debug!(imbalance = %imbalance, "imbalance SELL (transition)");
        }
    }
}
