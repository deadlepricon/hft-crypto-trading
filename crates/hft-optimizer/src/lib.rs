//! Strategy parameter optimization framework.
//!
//! Supports grid search, random search, and optional Bayesian optimization
//! to tune strategy parameters (spread thresholds, order size, entry/exit conditions, etc.).

mod grid;
mod params;
mod random;
mod runner;

pub use grid::GridSearch;
pub use params::{ParameterSpace, ParamValue};
pub use random::RandomSearch;
pub use runner::{OptimizationResult, OptimizationRunner};
