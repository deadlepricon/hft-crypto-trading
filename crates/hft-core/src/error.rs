//! Common error type for the HFT system.
//!
//! All crates should use this error type (or map into it) so the main binary
//! can handle failures consistently.

use thiserror::Error;

/// Top-level error type for the trading system.
#[derive(Error, Debug)]
pub enum HftError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network/connection error: {0}")]
    Network(String),

    #[error("Exchange API error: {0}")]
    Exchange(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Order rejected: {0}")]
    OrderRejected(String),

    #[error("Risk check failed: {0}")]
    RiskRejected(String),

    #[error("Invalid state or data: {0}")]
    InvalidState(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

/// Result alias using [HftError].
pub type Result<T> = std::result::Result<T, HftError>;
