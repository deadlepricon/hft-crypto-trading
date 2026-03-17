//! # hft-strategy
//!
//! Strategy engine: subscribes to feed handler events (order book, trades),
//! runs one or more strategy modules, and emits trade signals. Signals are
//! sent to the risk layer for validation before execution.

mod engine;
pub mod registry;
pub mod strategies;

pub use engine::StrategyEngine;
pub use registry::{create_strategy, create_strategies, strategy_names};
pub use strategies::{
    order_request, ImbalanceParams, ImbalanceStrategy, MarketMakerParams, MarketMakerStrategy,
    OrderWithStrategy, Signal, SignalIntent, Strategy, StrategyFill,
};
