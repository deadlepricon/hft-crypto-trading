//! # hft-risk
//!
//! Risk management layer: receives signals from the strategy engine, validates
//! against position limits and maximum exposure, and forwards approved orders
//! to the execution engine. Rejects invalid or dangerous orders.

mod manager;

pub use manager::RiskManager;
