//! # hft-backtesting
//!
//! Backtesting system: load historical market data (e.g. order book snapshots and
//! trades), replay them through the order book and feed handler, run the
//! strategy engine, and record PnL and trade statistics. No live exchange connection.

mod runner;

pub use runner::BacktestRunner;
