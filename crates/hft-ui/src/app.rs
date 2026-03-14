//! App state and layout for the TUI.
//!
//! Holds references to order book, metrics, and buffers for trades/logs
//! that the widgets read from.

use hft_metrics::Metrics;
use hft_order_book::OrderBook;
use std::collections::VecDeque;
use std::sync::Arc;

/// Maximum number of recent trades to show.
const MAX_RECENT_TRADES: usize = 50;
/// Maximum number of log lines.
const MAX_LOG_LINES: usize = 200;
/// Maximum number of price feed lines (book + trade prices).
const MAX_PRICE_FEED_LINES: usize = 40;
/// Max trade PnLs to keep for Sharpe ratio (rolling).
const MAX_TRADE_PNLS: usize = 1000;
/// Clamp PnL values so we never store or display crazy numbers (e.g. 2e18).
const PNL_CLAMP: f64 = 1e10;

fn sanitize_pnl(x: f64) -> f64 {
    if !x.is_finite() || x.abs() > PNL_CLAMP {
        0.0
    } else {
        x.clamp(-PNL_CLAMP, PNL_CLAMP)
    }
}

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
    /// Cumulative realized PnL (live profit).
    pub cumulative_pnl: f64,
    /// Win count for overall win rate.
    pub wins: u64,
    /// Loss count for overall win rate.
    pub losses: u64,
    /// Buy-side wins (for buy win %).
    pub buy_wins: u64,
    /// Buy-side losses.
    pub buy_losses: u64,
    /// Sell-side wins (for sell win %).
    pub sell_wins: u64,
    /// Sell-side losses.
    pub sell_losses: u64,
    /// Order book depth to display.
    pub book_depth: usize,
    /// Recent prices from feed (book best bid/ask + trade prices) for debugging.
    pub price_feed_lines: VecDeque<String>,
    /// Strategy comparison (from backtest/experiment results): name, PnL, win rate, drawdown, best params.
    pub strategy_comparison: Vec<StrategyComparisonLine>,
    /// Per-trade PnLs for Sharpe ratio (rolling, capped).
    pub trade_pnls: VecDeque<f64>,
    /// Peak cumulative PnL (for max drawdown).
    pub peak_pnl: f64,
    /// Maximum drawdown (peak - trough) so far.
    pub max_drawdown: f64,
    /// Time of first recorded trade (for profit per minute).
    pub first_trade_time: Option<std::time::Instant>,
    /// Total fills (our trades) received; updates on every record_trade_result.
    pub total_fills: u64,
    /// Latest unrealized PnL from position tracker (updated on each fill).
    pub unrealized_pnl: f64,
}

/// One row for the strategy comparison dashboard (backtest/optimization results).
#[derive(Clone, Debug)]
pub struct StrategyComparisonLine {
    pub strategy_name: String,
    pub pnl: f64,
    pub win_rate_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe: f64,
    pub best_params: String,
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
            buy_wins: 0,
            buy_losses: 0,
            sell_wins: 0,
            sell_losses: 0,
            book_depth: 20,
            price_feed_lines: VecDeque::new(),
            strategy_comparison: Vec::new(),
            trade_pnls: VecDeque::new(),
            peak_pnl: 0.0,
            max_drawdown: 0.0,
            first_trade_time: None,
            total_fills: 0,
            unrealized_pnl: 0.0,
        }
    }

    /// Set strategy comparison data (e.g. from experiment runner or loaded JSONL).
    pub fn set_strategy_comparison(&mut self, rows: Vec<StrategyComparisonLine>) {
        self.strategy_comparison = rows;
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

    /// Push a price feed line (book or trade price) for the "Price feed" panel.
    pub fn push_price_feed(&mut self, line: String) {
        if self.price_feed_lines.len() >= MAX_PRICE_FEED_LINES {
            self.price_feed_lines.pop_front();
        }
        self.price_feed_lines.push_back(line);
    }

    /// Overall win rate in [0.0, 1.0].
    pub fn win_rate(&self) -> f64 {
        let total = self.wins + self.losses;
        if total == 0 {
            0.0
        } else {
            self.wins as f64 / total as f64
        }
    }

    /// Buy win rate in [0.0, 1.0].
    pub fn buy_win_rate(&self) -> f64 {
        let total = self.buy_wins + self.buy_losses;
        if total == 0 {
            0.0
        } else {
            self.buy_wins as f64 / total as f64
        }
    }

    /// Sell win rate in [0.0, 1.0].
    pub fn sell_win_rate(&self) -> f64 {
        let total = self.sell_wins + self.sell_losses;
        if total == 0 {
            0.0
        } else {
            self.sell_wins as f64 / total as f64
        }
    }

    /// Record a paper/simulated or real trade result. Call from execution (paper fills or live fills).
    /// Only counts wins/losses when PnL is realized (pnl_delta != 0, e.g. on close); opens (pnl_delta == 0) update metrics but don't count as win or loss.
    /// `unrealized_pnl` is the position's unrealized PnL after this fill (so Live PnL = cumulative + unrealized).
    pub fn record_trade_result(&mut self, pnl_delta: f64, is_buy: bool, unrealized_pnl: f64) {
        self.total_fills += 1;
        let pnl_delta = sanitize_pnl(pnl_delta);
        let unrealized_pnl = sanitize_pnl(unrealized_pnl);
        self.unrealized_pnl = unrealized_pnl;
        if self.first_trade_time.is_none() {
            self.first_trade_time = Some(std::time::Instant::now());
        }
        self.cumulative_pnl = sanitize_pnl(self.cumulative_pnl + pnl_delta);
        if self.cumulative_pnl > self.peak_pnl {
            self.peak_pnl = self.cumulative_pnl;
        }
        self.peak_pnl = self.peak_pnl.clamp(-PNL_CLAMP, PNL_CLAMP);
        let drawdown = (self.peak_pnl - self.cumulative_pnl).max(0.0);
        if drawdown > self.max_drawdown && drawdown.is_finite() {
            self.max_drawdown = drawdown.clamp(0.0, PNL_CLAMP);
        }
        self.trade_pnls.push_back(pnl_delta);
        while self.trade_pnls.len() > MAX_TRADE_PNLS {
            self.trade_pnls.pop_front();
        }
        // Only count win/loss when we realize PnL (closing trade); opens have pnl_delta == 0
        if pnl_delta != 0.0 {
            let is_win = pnl_delta > 0.0;
            if is_win {
                self.wins += 1;
                if is_buy {
                    self.buy_wins += 1;
                } else {
                    self.sell_wins += 1;
                }
            } else {
                self.losses += 1;
                if is_buy {
                    self.buy_losses += 1;
                } else {
                    self.sell_losses += 1;
                }
            }
        }
    }

    /// Profit per trade (average over closed trades). Zero if no closed trades.
    pub fn profit_per_trade(&self) -> f64 {
        let total = self.wins + self.losses;
        if total == 0 {
            0.0
        } else {
            let x = self.cumulative_pnl / total as f64;
            if x.is_finite() { x } else { 0.0 }
        }
    }

    /// Sharpe ratio (annualized) from rolling trade PnLs. Zero if &lt; 2 trades.
    pub fn sharpe_ratio(&self) -> f64 {
        if self.trade_pnls.len() < 2 {
            return 0.0;
        }
        let mean: f64 = self.trade_pnls.iter().sum::<f64>() / self.trade_pnls.len() as f64;
        let variance = self
            .trade_pnls
            .iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>()
            / (self.trade_pnls.len() - 1) as f64;
        let std = variance.sqrt();
        if std < 1e-20 || !std.is_finite() {
            return 0.0;
        }
        let s = (mean / std) * (252.0_f64).sqrt();
        if s.is_finite() { s } else { 0.0 }
    }

    /// Profit per minute since first trade. Zero if no trades or &lt; 1 minute.
    pub fn profit_per_minute(&self) -> f64 {
        let start = match self.first_trade_time {
            Some(t) => t,
            None => return 0.0,
        };
        let elapsed_secs = start.elapsed().as_secs_f64();
        if elapsed_secs < 1.0 {
            return 0.0;
        }
        let p = self.cumulative_pnl / (elapsed_secs / 60.0);
        if p.is_finite() { p } else { 0.0 }
    }
}
