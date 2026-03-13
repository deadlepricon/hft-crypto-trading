//! Backtest runner: replay events and run strategy.
//!
//! Accepts a stream of historical [OrderBookSnapshot] and [TradeEvent] (e.g. from
//! a file or in-memory buffer), pushes them through the order book, and runs
//! the strategy. Records signals and optional PnL for analysis.

use hft_core::events::{OrderBookSnapshot, TradeEvent};
use hft_order_book::OrderBook;
use std::sync::Arc;
use tracing::info;

/// Configuration for a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub symbol: String,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

/// Runner that replays historical data and evaluates strategies.
pub struct BacktestRunner {
    config: BacktestConfig,
    order_book: Arc<OrderBook>,
}

impl BacktestRunner {
    /// Create a runner with the given config and order book.
    pub fn new(config: BacktestConfig, order_book: Arc<OrderBook>) -> Self {
        Self { config, order_book }
    }

    /// Apply a historical order book snapshot to the in-memory book.
    pub fn apply_snapshot(&self, snapshot: OrderBookSnapshot) {
        self.order_book.replace(snapshot.bids, snapshot.asks);
    }

    /// Apply a single trade event (for strategies that use trade stream).
    pub fn apply_trade(&self, _trade: TradeEvent) {
        // Order book may not change from trade-only; strategy can still react.
    }

    /// Run the backtest: in a full impl, this would read a data source,
    /// replay all events in order, run the strategy, and return results.
    pub async fn run(&self) -> BacktestResult {
        info!(
            symbol = %self.config.symbol,
            "backtest run (stub: no data source)"
        );
        BacktestResult::default()
    }
}

/// Result of a backtest run.
#[derive(Debug, Default)]
pub struct BacktestResult {
    pub total_pnl: f64,
    pub win_count: u64,
    pub loss_count: u64,
    pub total_trades: u64,
}

impl BacktestResult {
    /// Win rate as a fraction in [0.0, 1.0].
    pub fn win_rate(&self) -> f64 {
        if self.total_trades == 0 {
            0.0
        } else {
            self.win_count as f64 / self.total_trades as f64
        }
    }
}
