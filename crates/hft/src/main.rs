//! HFT Crypto Trading System — main binary.
//!
//! Single pipeline: feed → strategy → risk → execution. Switch paper/live with
//! **PAPER_TRADING** env (default: true). Same code path; only execution hits
//! the exchange in live mode.

use hft_core::OrderType;
use hft_exchange::{create_connector_with_env, ExchangeBackend};
use hft_feed_handler::FeedHandler;
use hft_metrics::Metrics;
use hft_order_book::OrderBook;
use hft_strategy::{
    create_strategies, strategy_names, OrderWithStrategy, StrategyEngine, StrategyFill,
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

    // Redirect all tracing output to the log file so it never writes to stderr/stdout
    // and corrupts the ratatui TUI (which uses raw mode + alternate screen).
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("hft_ui.log")?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false)
        .init();

    let paper_trading = env::var("PAPER_TRADING")
        .map(|v| {
            let v = v.trim();
            !(v.eq_ignore_ascii_case("false") || v == "0" || v.eq_ignore_ascii_case("no"))
        })
        .unwrap_or(true);

    let exchange = env::var("EXCHANGE").unwrap_or_else(|_| "simulator".to_string());
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

    let paper_submit_connector = if paper_trading && matches!(backend, ExchangeBackend::Simulator) {
        Some(Arc::clone(&feed_connector))
    } else {
        None
    };

    let (fill_event_tx, fill_event_rx) = if paper_trading && matches!(backend, ExchangeBackend::Simulator) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let (feed_handler, feed_tx) = FeedHandler::new(
        feed_connector,
        Arc::clone(&order_book),
        4096,
        fill_event_tx,
    );
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

    // Strategy selection: default is market maker; use STRATEGIES=imbalance or STRATEGIES=market_maker,imbalance to switch or run both.
    let strategy_names_input: Vec<String> = env::var("STRATEGIES")
        .unwrap_or_else(|_| "market_maker".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let (strategies, created_names) =
        create_strategies(Arc::clone(&order_book), &symbol, &strategy_names_input);
    if strategies.is_empty() {
        eprintln!(
            "error: no valid strategies. STRATEGIES={:?} — supported: {:?}",
            strategy_names_input,
            strategy_names()
        );
        std::process::exit(1);
    }
    if created_names.len() < strategy_names_input.len() {
        tracing::warn!(
            requested = ?strategy_names_input,
            created = ?created_names,
            "some strategy names were unknown and skipped"
        );
    }
    let mut engine = StrategyEngine::new(signal_tx);
    for s in strategies {
        engine.add_strategy(s);
    }

    let risk_limits = hft_risk::RiskLimits::default();
    let risk = Arc::new(hft_risk::RiskManager::new(risk_limits, order_tx));

    let (fill_tx_ui, fill_rx_ui_channel): (
        mpsc::UnboundedSender<hft_execution::PaperFill>,
        _,
    ) = mpsc::unbounded_channel();
    let (fill_tx_strategy, fill_rx_strategy): (
        mpsc::UnboundedSender<StrategyFill>,
        _,
    ) = mpsc::unbounded_channel();
    let position_tracker: Arc<dyn hft_execution::PositionTracker> = Arc::new(PaperPositionTracker::new());

    let mut execution = if paper_trading {
        hft_execution::ExecutionEngine::new_paper(
            Arc::clone(&order_book),
            order_rx,
            fill_tx_ui.clone(),
            Some(fill_tx_strategy.clone()),
            Arc::clone(&position_tracker),
            paper_submit_connector,
        )
    } else {
        if matches!(backend, ExchangeBackend::Binance) {
            tracing::error!(
                "LIVE MODE with BinanceConnector: order submission is NOT implemented. \
                 All orders will fail silently. Set PAPER_TRADING=true or implement \
                 Binance REST signing in hft-exchange/src/binance.rs before going live."
            );
        }
        let execution_connector = create_connector_with_env(backend, symbol.clone());
        hft_execution::ExecutionEngine::new_live(execution_connector, order_rx)
    };

    // Wire execution → risk position feedback so limits track real fills.
    let (risk_pos_tx, mut risk_pos_rx) = mpsc::channel::<(String, f64)>(256);
    let risk_pos_tx_fill_proc = risk_pos_tx.clone();
    execution.set_risk_position_tx(risk_pos_tx);

    let run_fill_processor = async move {
        if let Some(mut fill_event_rx) = fill_event_rx {
            while let Some(fill) = fill_event_rx.recv().await {
                let req = hft_core::OrderRequest {
                    symbol: fill.symbol.clone(),
                    side: fill.side,
                    order_type: OrderType::Limit,
                    qty: fill.qty,
                    price: Some(fill.price),
                    time_in_force: None,
                    client_order_id: fill.client_order_id.clone(),
                };
                let (pnl_delta, is_buy, unrealized_pnl, qty_after, entry_price_after) =
                    position_tracker.apply_fill(&req, fill.price);
                // Update risk manager position so limits track simulator fills.
                let qty_f: f64 = req.qty.to_string().parse().unwrap_or(0.0);
                let delta = if matches!(req.side, hft_core::OrderSide::Buy) { qty_f } else { -qty_f };
                let _ = risk_pos_tx_fill_proc.send((req.symbol.clone(), delta)).await;
                let paper_fill = hft_execution::PaperFill {
                    request: req,
                    fill_price: fill.price,
                    pnl_delta,
                    is_buy,
                    unrealized_pnl,
                    qty_after,
                    entry_price_after,
                };
                let _ = fill_tx_ui.send(paper_fill);
                match fill.client_order_id {
                    Some(strategy_id) => {
                        let _ = fill_tx_strategy.send(StrategyFill {
                            strategy_id,
                            symbol: fill.symbol,
                            side: fill.side,
                            filled_qty: fill.qty,
                            fill_price: fill.price,
                        });
                    }
                    None => {
                        tracing::warn!(
                            symbol = %fill.symbol,
                            "simulator fill missing client_order_id; strategy fill feedback skipped"
                        );
                    }
                }
            }
        } else {
            std::future::pending::<()>().await;
        }
    };

    let risk_for_run = Arc::clone(&risk);
    let risk_for_pos = Arc::clone(&risk);

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
            let run_risk = async move {
                let _ = risk_for_run.run(signal_rx).await;
            };
            let run_exec = async {
                let mut exec = execution;
                let _ = exec.run().await;
            };
            let run_risk_pos = async move {
                while let Some((symbol, delta)) = risk_pos_rx.recv().await {
                    risk_for_pos.update_position(&symbol, delta);
                }
            };
            tokio::join!(run_engine, run_risk, run_exec, run_fill_processor, run_risk_pos);
        });
    });

    let mut app = App::new(Arc::clone(&order_book), Arc::clone(&metrics));
    app.push_log(format!(
        "Exchange: {} | Symbol: {} | Mode: {}",
        backend_name(backend),
        symbol,
        if paper_trading { "PAPER" } else { "LIVE" }
    ));
    app.push_log(format!(
        "Strategies: {} (set STRATEGIES=comma,separated to switch or run multiple)",
        created_names.join(", ")
    ));
    app.push_log(if paper_trading {
        if matches!(backend, ExchangeBackend::Simulator) {
            "Paper trading: orders sent to simulator; fills from WebSocket (channel orders/fill).".to_string()
        } else {
            "Paper trading: orders simulated at mid; no exchange submission.".to_string()
        }
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
