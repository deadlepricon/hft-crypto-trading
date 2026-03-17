//! Backtest runner: replay feed events, run strategies, simulate execution, record trades.
//!
//! Replays a stream of (timestamp, FeedEvent), updates the order book, dispatches
//! to strategies, and simulates order fills to produce a list of trades and a
//! [BacktestResult] with optional [PerformanceReport].

use chrono::{DateTime, Utc};
use hft_core::events::{EventEnvelope, EventSource};
use hft_core::OrderSide;
use hft_feed_handler::FeedEvent;
use hft_order_book::OrderBook;
use hft_performance_metrics::{PerformanceMetrics, PerformanceReport, SimulatedTrade, TradeSide};
use hft_strategy::{Signal, Strategy};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Configuration for a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub symbol: String,
    /// Optional: only replay events in this range.
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    /// Speed multiplier: 1.0 = real-time, 0 = no delay (run as fast as possible).
    pub speed_multiplier: f64,
    /// Risk-free rate for Sharpe (e.g. 0.0).
    pub risk_free_rate: f64,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            symbol: "BTCUSDT".to_string(),
            start_ts: None,
            end_ts: None,
            speed_multiplier: 0.0, // no delay by default
            risk_free_rate: 0.0,
        }
    }
}

/// A single event to replay (timestamp + payload).
#[derive(Debug, Clone)]
pub struct ReplayEvent {
    pub ts: DateTime<Utc>,
    pub event: FeedEvent,
}

/// Half of an open round-trip trade, waiting to be closed by the opposing signal.
struct OpenPosition {
    entry_price: Decimal,
    qty: Decimal,
    entry_time: DateTime<Utc>,
    side: TradeSide,
}

/// Runner that replays historical data, runs strategies, and simulates execution.
pub struct BacktestRunner {
    config: BacktestConfig,
    order_book: Arc<OrderBook>,
}

impl BacktestRunner {
    /// Create a runner with the given config and order book.
    pub fn new(config: BacktestConfig, order_book: Arc<OrderBook>) -> Self {
        Self { config, order_book }
    }

    /// Apply a feed event to the order book (for replay).
    pub fn apply_feed_event(&self, event: &FeedEvent) {
        match event {
            FeedEvent::OrderBookSnapshot(s) => {
                self.order_book.replace(s.bids.clone(), s.asks.clone());
            }
            FeedEvent::OrderBookDelta(d) => {
                let bid_tuples: Vec<_> = d.bids.iter().map(|l| (l.price, l.qty)).collect();
                let ask_tuples: Vec<_> = d.asks.iter().map(|l| (l.price, l.qty)).collect();
                if !bid_tuples.is_empty() {
                    self.order_book.update_bids(&bid_tuples);
                }
                if !ask_tuples.is_empty() {
                    self.order_book.update_asks(&ask_tuples);
                }
            }
            FeedEvent::Trade(_) | FeedEvent::Ticker(_) => {}
        }
    }

    /// Run backtest by replaying `events` and running `strategies`. Signals are
    /// simulated as fills at current mid (or best bid/ask). Returns result with
    /// trades and performance report.
    pub async fn run(
        &self,
        events: impl IntoIterator<Item = ReplayEvent>,
        strategies: &[Arc<dyn Strategy>],
    ) -> BacktestResult {
        let (signal_tx, mut signal_rx) = mpsc::channel::<Signal>(1024);
        let mut trades: Vec<SimulatedTrade> = Vec::new();
        // Tracks open (unmatched) positions; buys wait for a sell, shorts wait for a buy.
        let mut open_positions: HashMap<String, OpenPosition> = HashMap::new();
        let mut last_ts = None::<DateTime<Utc>>;
        let speed = self.config.speed_multiplier;

        for replay in events {
            let envelope = EventEnvelope {
                source: EventSource::FeedHandler,
                ts: replay.ts,
                payload: replay.event.clone(),
            };

            self.apply_feed_event(&envelope.payload);

            for s in strategies {
                s.on_feed_event(&envelope.payload, &signal_tx);
            }

            if speed > 0.0 && last_ts.is_some() {
                let prev = last_ts.unwrap();
                let dt_ms = (replay.ts - prev).num_milliseconds().max(0) as u64;
                let delay_ms = (dt_ms as f64 / speed) as u64;
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
            last_ts = Some(replay.ts);

            while let Ok(signal) = signal_rx.try_recv() {
                if let Some(sim) = self.process_signal(&signal, envelope.ts, &mut open_positions) {
                    trades.push(sim);
                }
            }
        }

        drop(signal_tx);
        while let Some(signal) = signal_rx.recv().await {
            if let Some(sim) = self.process_signal(&signal, Utc::now(), &mut open_positions) {
                trades.push(sim);
            }
        }

        let report = PerformanceMetrics::compute(&trades, self.config.risk_free_rate);
        BacktestResult {
            total_pnl: report.total_pnl,
            win_count: report.win_count,
            loss_count: report.loss_count,
            total_trades: report.total_trades,
            trades,
            performance: report,
        }
    }

    /// Realistic fill price: buys pay the ask (cost of crossing the spread);
    /// sells receive the bid. Falls back to mid, then the signal's own limit price.
    fn fill_price_for_signal(&self, signal: &Signal) -> Option<Decimal> {
        let req = &signal.request;
        match req.side {
            OrderSide::Buy => self
                .order_book
                .best_ask()
                .or_else(|| self.order_book.best_bid())
                .or(req.price),
            OrderSide::Sell => self
                .order_book
                .best_bid()
                .or_else(|| self.order_book.best_ask())
                .or(req.price),
        }
    }

    /// Pair a signal against the open-position map.
    ///
    /// - Opening fills (no opposing position) are recorded in `open_positions` and return `None`.
    /// - Closing fills (opposing position exists) produce a [SimulatedTrade] with real entry/exit
    ///   prices and return `Some(trade)`.
    fn process_signal(
        &self,
        signal: &Signal,
        fill_ts: DateTime<Utc>,
        open_positions: &mut HashMap<String, OpenPosition>,
    ) -> Option<SimulatedTrade> {
        let req = &signal.request;
        let fill_price = self.fill_price_for_signal(signal)?;
        let symbol = req.symbol.clone();
        let qty = req.qty;

        match req.side {
            OrderSide::Buy => {
                // If we hold a short, this buy closes (part of) it → realize PnL.
                if let Some(pos) = open_positions.get(&symbol) {
                    if pos.side == TradeSide::Sell {
                        let close_qty = qty.min(pos.qty);
                        let trade = SimulatedTrade {
                            entry_time: pos.entry_time,
                            exit_time: fill_ts,
                            side: TradeSide::Sell,
                            entry_price: pos.entry_price,
                            exit_price: fill_price,
                            qty: close_qty,
                        };
                        let remaining = pos.qty - close_qty;
                        if remaining <= Decimal::ZERO {
                            open_positions.remove(&symbol);
                        } else {
                            open_positions.get_mut(&symbol).unwrap().qty = remaining;
                        }
                        return Some(trade);
                    }
                }
                // No opposing position: open or add to long.
                let pos = open_positions.entry(symbol).or_insert(OpenPosition {
                    entry_price: fill_price,
                    qty: Decimal::ZERO,
                    entry_time: fill_ts,
                    side: TradeSide::Buy,
                });
                if pos.qty > Decimal::ZERO {
                    // Weighted-average entry when adding to an existing long.
                    let new_qty = pos.qty + qty;
                    pos.entry_price =
                        (pos.entry_price * pos.qty + fill_price * qty) / new_qty;
                } else {
                    pos.entry_price = fill_price;
                    pos.entry_time = fill_ts;
                }
                pos.qty += qty;
                None
            }
            OrderSide::Sell => {
                // If we hold a long, this sell closes (part of) it → realize PnL.
                if let Some(pos) = open_positions.get(&symbol) {
                    if pos.side == TradeSide::Buy {
                        let close_qty = qty.min(pos.qty);
                        let trade = SimulatedTrade {
                            entry_time: pos.entry_time,
                            exit_time: fill_ts,
                            side: TradeSide::Buy,
                            entry_price: pos.entry_price,
                            exit_price: fill_price,
                            qty: close_qty,
                        };
                        let remaining = pos.qty - close_qty;
                        if remaining <= Decimal::ZERO {
                            open_positions.remove(&symbol);
                        } else {
                            open_positions.get_mut(&symbol).unwrap().qty = remaining;
                        }
                        return Some(trade);
                    }
                }
                // No opposing position: open or add to short.
                let pos = open_positions.entry(symbol).or_insert(OpenPosition {
                    entry_price: fill_price,
                    qty: Decimal::ZERO,
                    entry_time: fill_ts,
                    side: TradeSide::Sell,
                });
                if pos.qty > Decimal::ZERO {
                    let new_qty = pos.qty + qty;
                    pos.entry_price =
                        (pos.entry_price * pos.qty + fill_price * qty) / new_qty;
                } else {
                    pos.entry_price = fill_price;
                    pos.entry_time = fill_ts;
                }
                pos.qty += qty;
                None
            }
        }
    }

    /// Run with no events (stub); returns empty result. Useful for wiring tests.
    pub async fn run_empty(&self, strategies: &[Arc<dyn Strategy>]) -> BacktestResult {
        self.run(std::iter::empty::<ReplayEvent>(), strategies).await
    }
}

/// Result of a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub total_pnl: f64,
    pub win_count: u64,
    pub loss_count: u64,
    pub total_trades: u64,
    pub trades: Vec<SimulatedTrade>,
    pub performance: PerformanceReport,
}

impl Default for BacktestResult {
    fn default() -> Self {
        Self {
            total_pnl: 0.0,
            win_count: 0,
            loss_count: 0,
            total_trades: 0,
            trades: Vec::new(),
            performance: PerformanceReport::default(),
        }
    }
}

impl BacktestResult {
    pub fn win_rate(&self) -> f64 {
        if self.total_trades == 0 {
            0.0
        } else {
            self.win_count as f64 / self.total_trades as f64
        }
    }

    pub fn performance_report(&self) -> &PerformanceReport {
        &self.performance
    }
}
