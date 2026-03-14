//! # hft-execution
//!
//! Execution engine: receives approved orders from the risk manager, submits
//! them via the exchange connector, and tracks order lifecycle (submit, fill,
//! cancel). Reports fills and position updates to the risk layer and UI.

mod engine;

pub use engine::{ExecutionEngine, ExecutionMode, PaperFill, PositionTracker};
