//! HFT Crypto Trading System — main binary.
//!
//! Single pipeline: feed → strategy → risk → execution. Switch paper/live with
//! **PAPER_TRADING** env (default: true). Same code path; only execution hits
//! the exchange in live mode.

use hft_exchange::{create_connector_with_env, ExchangeBackend};
use hft_feed_handler::FeedHandler;
use hft_metrics::Metrics;
use hft_order_book::OrderBook;
use hft_strategy::{
    MarketMakerParams, MarketMakerStrategy, OrderWithStrategy, StrategyEngine, StrategyFill,
};
use hft_ui::{run_ui, App};
use std::env;
use std::io::IsTerminal;
use std::sync::Arc;
use std::thread;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

mod position_tracker;

use position_tracker::PaperPositionTracker;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::io::stdout().is_terminal() {
        eprintln!(
            "error: this app needs a real terminal (TTY).\n\
             Run it from a terminal (e.g. Terminal.app, iTerm, or your OS terminal), not from an IDE run panel."
        );
        std::process::exit(1);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let paper_trading = env::var("PAPER_TRADING")
        .map(|v| {
            let v = v.trim();
            !(v.eq_ignore_ascii_case("false") || v == "0" || v.eq_ignore_ascii_case("no"))
        })
        .unwrap_or(true);

    let exchange = env::var("EXCHANGE").unwrap_or_else(|_| "binance".to_string());
    let symbol = env::var("SYMBOL").unwrap_or_else(|_| {
        if exchange.eq_ignore_ascii_case("binance") {
            "btcusdt".to_string()
        } else {
            "BTC/USDT".to_string()
        }
    });

    let backend: ExchangeBackend = exchange.parse().unwrap_or_default();
    let feed_connector = create_connector_with_env(backend, symbol.clone());

    let order_book = Arc::new(OrderBook::new(symbol.clone()));
    let metrics = Arc::new(Metrics::new());

    let (feed_handler, feed_tx) = FeedHandler::new(feed_connector, Arc::clone(&order_book), 1024);
    let feed_rx_ui = feed_tx.subscribe();
    let feed_rx_engine = feed_tx.subscribe();

    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "failed to create tokio runtime");
                return;
            }
        };
        rt.block_on(async {
            if let Err(e) = feed_handler.run().await {
                tracing::error!(error = %e, "feed handler stopped");
            }
        });
    });

    let (signal_tx, signal_rx) = mpsc::channel::<hft_strategy::Signal>(1024);
    let (order_tx, order_rx) = mpsc::channel::<OrderWithStrategy>(1024);

    let params = {
        let mut p = MarketMakerParams::default();
        p.symbol = symbol.clone();
        p.qty_per_order = rust_decimal::Decimal::new(1, 3); // 0.001
        p.spread_bps = 10;
        p.max_inventory = rust_decimal::Decimal::new(10, 3);
        p.imbalance_skew_factor = rust_decimal::Decimal::new(5, 0);
        p.book_depth = 10;
        p.min_tick_move = rust_decimal::Decimal::new(1, 2); // 0.01
        p
    };
    let strategy = Arc::new(MarketMakerStrategy::new(Arc::clone(&order_book), params));
    let mut engine = StrategyEngine::new(signal_tx);
    engine.add_strategy(strategy);

    let risk_limits = hft_risk::RiskLimits::default();
    let risk = hft_risk::RiskManager::new(risk_limits, order_tx);

    let (fill_tx_ui, fill_rx_ui_channel): (
        mpsc::UnboundedSender<hft_execution::PaperFill>,
        _,
    ) = mpsc::unbounded_channel();
    let (fill_tx_strategy, fill_rx_strategy): (
        mpsc::UnboundedSender<StrategyFill>,
        _,
    ) = mpsc::unbounded_channel();
    let position_tracker: Arc<dyn hft_execution::PositionTracker> = Arc::new(PaperPositionTracker::new());

    let execution = if paper_trading {
        hft_execution::ExecutionEngine::new_paper(
            Arc::clone(&order_book),
            order_rx,
            fill_tx_ui,
            Some(fill_tx_strategy),
            position_tracker,
        )
    } else {
        let execution_connector = create_connector_with_env(backend, symbol.clone());
        hft_execution::ExecutionEngine::new_live(execution_connector, order_rx)
    };

    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "strategy runtime failed");
                return;
            }
        };
        rt.block_on(async {
            let run_engine = async {
                engine.run(feed_rx_engine, Some(fill_rx_strategy)).await;
            };
            let run_risk = async {
                let _ = risk.run(signal_rx).await;
            };
            let run_exec = async {
                let mut exec = execution;
                let _ = exec.run().await;
            };
            tokio::join!(run_engine, run_risk, run_exec);
        });
    });

    let mut app = App::new(Arc::clone(&order_book), Arc::clone(&metrics));
    app.push_log(format!(
        "Exchange: {} | Symbol: {} | Mode: {}",
        backend_name(backend),
        symbol,
        if paper_trading { "PAPER" } else { "LIVE" }
    ));
    app.push_log(if paper_trading {
        "Paper trading: orders simulated at mid; no exchange submission.".to_string()
    } else {
        "Live trading: orders sent to exchange.".to_string()
    });

    let fill_rx_for_ui = if paper_trading {
        Some(fill_rx_ui_channel)
    } else {
        None
    };
    if let Err(e) = run_ui(app, Some(feed_rx_ui), fill_rx_for_ui) {
        eprintln!("UI error: {}", e);
        if e.to_string().contains("raw mode") || e.to_string().contains("terminal") {
            eprintln!("Hint: run this from a real terminal (TTY), not from an IDE.");
        }
        std::process::exit(1);
    }

    Ok(())
}

fn backend_name(b: ExchangeBackend) -> &'static str {
    match b {
        ExchangeBackend::Simulator => "simulator",
        ExchangeBackend::Binance => "binance.us",
    }
}
