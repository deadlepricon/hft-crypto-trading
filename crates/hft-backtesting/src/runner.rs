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
                if let Some(sim) = self.simulate_fill(&signal, envelope.ts) {
                    trades.push(sim);
                }
            }
        }

        drop(signal_tx);
        while let Some(signal) = signal_rx.recv().await {
            if let Some(sim) = self.simulate_fill(&signal, Utc::now()) {
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

    /// Simulate fill at current mid (or best ask for buy, best bid for sell). Returns a [SimulatedTrade] if filled.
    fn simulate_fill(&self, signal: &Signal, fill_ts: DateTime<Utc>) -> Option<SimulatedTrade> {
        let req = &signal.request;
        let fill_price = match (self.order_book.best_bid(), self.order_book.best_ask()) {
            (Some(b), Some(a)) => req.price.unwrap_or_else(|| (b + a) / Decimal::from(2)),
            (Some(b), None) => req.price.unwrap_or(b),
            (None, Some(a)) => req.price.unwrap_or(a),
            (None, None) => return None,
        };
        let entry_ts = fill_ts - chrono::Duration::milliseconds(1);
        let side = match req.side {
            OrderSide::Buy => TradeSide::Buy,
            OrderSide::Sell => TradeSide::Sell,
        };
        Some(SimulatedTrade {
            entry_time: entry_ts,
            exit_time: fill_ts,
            side,
            entry_price: fill_price,
            exit_price: fill_price,
            qty: req.qty,
        })
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
