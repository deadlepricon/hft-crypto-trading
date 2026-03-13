//! # hft-strategy
//!
//! Strategy engine: subscribes to feed handler events (order book, trades),
//! runs one or more strategy modules, and emits trade signals. Signals are
//! sent to the risk layer for validation before execution.

mod engine;
pub mod strategies;

pub use engine::StrategyEngine;
pub use strategies::{Signal, Strategy};
