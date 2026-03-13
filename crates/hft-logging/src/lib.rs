//! # hft-logging
//!
//! Persist trades and key events to disk (or a store) for audit and analysis.
//! Provides a simple API to append trade/fill and order events.

mod persist;

pub use persist::Persist;
