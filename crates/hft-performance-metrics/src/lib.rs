//! Performance evaluation metrics for trading strategies.
//!
//! Computes PnL, win rate, Sharpe ratio, maximum drawdown, average trade duration,
//! and trade frequency from a list of trades or an equity curve.

mod metrics;
mod trade;

pub use metrics::{PerformanceReport, PerformanceMetrics};
pub use trade::{SimulatedTrade, TradeOutcome, TradeSide};
