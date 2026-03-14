//! # hft-exchange
//!
//! Exchange-specific connectors implementing a common [ExchangeConnector] trait.
//! Handles WebSocket streams (depth, trades) and REST (order placement, cancel).
//! Use [ExchangeBackend] and [create_connector] so simulator and live run under the same code path.

mod binance;
mod connector;
mod simulator;

pub use binance::BinanceConnector;
pub use connector::{ExchangeConnector, ExchangeMessage};
pub use connector::ExchangeMessage::*;
pub use simulator::{SimulatorConnector, SIMULATOR_BASE_URL, SIMULATOR_WS_URL};

/// Which exchange backend to use. Same pipeline runs for both; only the connector changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExchangeBackend {
    /// Local simulator at http://localhost:8765, ws://localhost:8765/ws/feed
    Simulator,
    /// Live Binance WebSocket (default).
    #[default]
    Binance,
}

impl std::str::FromStr for ExchangeBackend {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "simulator" | "sim" => Ok(ExchangeBackend::Simulator),
            "binance" | "live" => Ok(ExchangeBackend::Binance),
            _ => Err(()),
        }
    }
}

/// Create an exchange connector for the given backend and symbol.
/// Use this so simulator and live data use exactly the same code path.
pub fn create_connector(
    backend: ExchangeBackend,
    symbol: impl Into<String>,
) -> std::sync::Arc<dyn ExchangeConnector> {
    create_connector_with_env(backend, symbol)
}

/// Like [create_connector], but reads optional env overrides:
/// - Simulator: `SIMULATOR_BASE_URL`, `SIMULATOR_WS_URL`
/// - Binance.US: `BINANCE_WS_URL` (optional; full URL override), `BINANCE_WS_BASE` (optional; default wss://stream.binance.us:9443)
pub fn create_connector_with_env(
    backend: ExchangeBackend,
    symbol: impl Into<String>,
) -> std::sync::Arc<dyn ExchangeConnector> {
    let symbol = symbol.into();
    match backend {
        ExchangeBackend::Simulator => {
            let base = std::env::var("SIMULATOR_BASE_URL").unwrap_or_else(|_| SIMULATOR_BASE_URL.to_string());
            let ws = std::env::var("SIMULATOR_WS_URL").unwrap_or_else(|_| SIMULATOR_WS_URL.to_string());
            std::sync::Arc::new(SimulatorConnector::with_urls(symbol, base, ws))
        }
        ExchangeBackend::Binance => {
            if let Ok(url) = std::env::var("BINANCE_WS_URL") {
                std::sync::Arc::new(BinanceConnector::with_ws_url(symbol, url))
            } else {
                std::sync::Arc::new(BinanceConnector::new(symbol))
            }
        }
    }
}
