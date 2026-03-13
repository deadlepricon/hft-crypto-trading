//! # hft-core
//!
//! Shared domain types, events, and errors used across all HFT system crates.
//! This crate has no I/O or async dependencies to keep it lightweight and
//! suitable for use in hot paths and backtesting.

pub mod error;
pub mod events;
pub mod types;

pub use error::{HftError, Result};
pub use events::*;
pub use types::*;
