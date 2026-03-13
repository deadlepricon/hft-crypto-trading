//! HFT Crypto Trading System — main binary.
//!
//! Wires together: exchange connector → feed handler → order book → UI.
//! Exchange is chosen by EXCHANGE env (simulator | binance); same code path for both.

use hft_exchange::{create_connector_with_env, ExchangeBackend};
use hft_feed_handler::FeedHandler;
use hft_metrics::Metrics;
use hft_order_book::OrderBook;
use hft_ui::{run_ui, App};
use std::env;
use std::sync::Arc;
use std::thread;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let exchange = env::var("EXCHANGE").unwrap_or_else(|_| "simulator".to_string());
    let symbol = env::var("SYMBOL").unwrap_or_else(|_| "BTC/USDT".to_string());

    let backend: ExchangeBackend = exchange.parse().unwrap_or_default();
    let connector = create_connector_with_env(backend, symbol.clone());

    let order_book = Arc::new(OrderBook::new(symbol.clone()));
    let metrics = Arc::new(Metrics::new());

    let (feed_handler, _feed_rx) = FeedHandler::new(connector, Arc::clone(&order_book), 1024);

    // Run feed handler in a background thread so the order book receives live data
    // while the UI runs. Same code path for simulator or live exchange.
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            if let Err(e) = feed_handler.run().await {
                tracing::error!(error = %e, "feed handler stopped");
            }
        });
    });

    let mut app = App::new(Arc::clone(&order_book), Arc::clone(&metrics));
    app.push_log(format!(
        "Exchange: {} | Symbol: {}",
        backend_name(backend),
        symbol
    ));
    app.push_log("Feed handler started. Connect simulator at ws://localhost:8765/ws/feed for simulator.".to_string());

    run_ui(app)?;

    Ok(())
}

fn backend_name(b: ExchangeBackend) -> &'static str {
    match b {
        ExchangeBackend::Simulator => "simulator",
        ExchangeBackend::Binance => "binance",
    }
}
