//! App state and layout for the TUI.
//!
//! Holds references to order book, metrics, and buffers for trades/logs
//! that the widgets read from.

use hft_metrics::Metrics;
use hft_order_book::OrderBook;
use std::sync::Arc;
use std::collections::VecDeque;

/// Maximum number of recent trades to show.
const MAX_RECENT_TRADES: usize = 50;
/// Maximum number of log lines.
const MAX_LOG_LINES: usize = 200;

/// Central app state for the TUI.
pub struct App {
    /// Order book for the main symbol (for order book widget).
    pub order_book: Arc<OrderBook>,
    /// Global metrics (latency, counts).
    pub metrics: Arc<Metrics>,
    /// Recent trades (symbol, price, qty, side) for display.
    pub recent_trades: VecDeque<TradeLine>,
    /// System log lines.
    pub log_lines: VecDeque<String>,
    /// Current positions (symbol -> (qty, entry_price, unrealized_pnl)).
    pub positions: Vec<PositionLine>,
    /// Cumulative realized PnL.
    pub cumulative_pnl: f64,
    /// Win count for win rate.
    pub wins: u64,
    /// Loss count for win rate.
    pub losses: u64,
    /// Order book depth to display.
    pub book_depth: usize,
}

/// One line for the recent trades list.
#[derive(Clone, Debug)]
pub struct TradeLine {
    pub symbol: String,
    pub price: String,
    pub qty: String,
    pub side: String,
}

/// One line for the positions list.
#[derive(Clone, Debug)]
pub struct PositionLine {
    pub symbol: String,
    pub qty: String,
    pub entry_price: String,
    pub unrealized_pnl: String,
}

impl App {
    /// Create app state with order book and metrics. Other buffers start empty.
    pub fn new(order_book: Arc<OrderBook>, metrics: Arc<Metrics>) -> Self {
        Self {
            order_book,
            metrics,
            recent_trades: VecDeque::new(),
            log_lines: VecDeque::new(),
            positions: Vec::new(),
            cumulative_pnl: 0.0,
            wins: 0,
            losses: 0,
            book_depth: 20,
        }
    }

    /// Push a log line (trimmed to capacity).
    pub fn push_log(&mut self, line: String) {
        if self.log_lines.len() >= MAX_LOG_LINES {
            self.log_lines.pop_front();
        }
        self.log_lines.push_back(line);
    }

    /// Push a recent trade.
    pub fn push_trade(&mut self, trade: TradeLine) {
        if self.recent_trades.len() >= MAX_RECENT_TRADES {
            self.recent_trades.pop_front();
        }
        self.recent_trades.push_back(trade);
    }

    /// Win rate in [0.0, 1.0].
    pub fn win_rate(&self) -> f64 {
        let total = self.wins + self.losses;
        if total == 0 {
            0.0
        } else {
            self.wins as f64 / total as f64
        }
    }
}
