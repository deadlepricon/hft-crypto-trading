//! TUI event loop and layout: run the terminal UI with order book, trades, PnL, logs.

use hft_core::events::EventEnvelope;
use hft_execution::PaperFill;
use hft_feed_handler::FeedEvent;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;
use ratatui::Terminal;
use std::io::{Stdout, Write};
use tokio::sync::{broadcast, mpsc};

/// Format decimal for log: trim to sensible precision (avoids 0.00040999999999999544).
fn fmt_qty(d: &rust_decimal::Decimal) -> String {
    d.round_dp(6).to_string()
}
fn fmt_price(d: &rust_decimal::Decimal) -> String {
    d.round_dp(2).to_string()
}

use crate::app::{App, TradeLine};
use crate::widgets::{
    logs_widget, order_book_widget, pnl_latency_widget, positions_widget, price_feed_widget,
    strategy_comparison_widget, trades_widget,
};

/// Run the TUI. If `feed_rx` is Some, drain feed events for price feed and market trades.
/// If `fill_rx` is Some (paper trading), drain our fills and update PnL / recent trades.
/// Exit with 'q' or Ctrl+C.
/// Logs are written to `hft_ui.log` in the current directory; run `tail -f hft_ui.log` in another terminal (start the app first so the file exists).
pub fn run_ui(
    mut app: App,
    feed_rx: Option<broadcast::Receiver<EventEnvelope<FeedEvent>>>,
    fill_rx: Option<mpsc::UnboundedReceiver<PaperFill>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("hft_ui.log");
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    eprintln!("Logs: {} (run 'tail -f hft_ui.log' in another terminal)", log_path.display());

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_loop(&mut terminal, &mut app, &mut log_file, feed_rx, fill_rx);

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn write_log(app: &mut App, log_file: &mut std::fs::File, line: &str) {
    app.push_log(line.to_string());
    let _ = writeln!(log_file, "{}", line);
    let _ = log_file.flush();
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    log_file: &mut std::fs::File,
    mut feed_rx: Option<broadcast::Receiver<EventEnvelope<FeedEvent>>>,
    mut fill_rx: Option<mpsc::UnboundedReceiver<PaperFill>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Drain paper fills first so PnL / Our fills update before we draw
        if let Some(rx) = fill_rx.as_mut() {
            while let Ok(fill) = rx.try_recv() {
                app.metrics.inc_fills();
                app.record_trade_result(fill.pnl_delta, fill.is_buy, fill.unrealized_pnl);
                // Update positions panel: remove flat positions, upsert live ones.
                let symbol = fill.request.symbol.clone();
                if fill.qty_after.abs() < 1e-9 {
                    app.positions.retain(|p| p.symbol != symbol);
                } else {
                    let new_line = crate::app::PositionLine {
                        symbol: symbol.clone(),
                        qty: format!("{:.6}", fill.qty_after),
                        entry_price: format!("{:.2}", fill.entry_price_after),
                        unrealized_pnl: format!("{:.4}", fill.unrealized_pnl),
                    };
                    if let Some(existing) = app.positions.iter_mut().find(|p| p.symbol == symbol) {
                        *existing = new_line;
                    } else {
                        app.positions.push(new_line);
                    }
                }
                let side_str = if fill.is_buy { "Buy" } else { "Sell" };
                app.push_trade(TradeLine {
                    symbol: fill.request.symbol.clone(),
                    price: fmt_price(&fill.fill_price),
                    qty: fmt_qty(&fill.request.qty),
                    side: side_str.to_string(),
                });
                let fill_log = format!(
                    "Fill #{}: {} {} qty={} @ {} | pnl_delta={:.4} cum={:.4} unrl={:.4} | W/L {}/{}",
                    app.total_fills,
                    side_str,
                    fill.request.symbol,
                    fmt_qty(&fill.request.qty),
                    fmt_price(&fill.fill_price),
                    fill.pnl_delta,
                    app.cumulative_pnl,
                    app.unrealized_pnl,
                    app.wins,
                    app.losses,
                );
                write_log(app, log_file, &fill_log);
            }
        }
        // Drain feed events (price feed + market trades). Handle Lagged so we don't stall.
        if let Some(rx) = feed_rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(env) => {
                        match &env.payload {
                            FeedEvent::OrderBookSnapshot(_) | FeedEvent::OrderBookDelta(_) => {
                                let best_bid = app.order_book.best_bid().map(|p| p.to_string()).unwrap_or_default();
                                let best_ask = app.order_book.best_ask().map(|p| p.to_string()).unwrap_or_default();
                                if !best_bid.is_empty() || !best_ask.is_empty() {
                                    app.push_price_feed(format!("book best_bid={} best_ask={}", best_bid, best_ask));
                                }
                            }
                            FeedEvent::Trade(t) => {
                                app.push_price_feed(format!("trade {:?} @ {} qty={}", t.side, fmt_price(&t.price), fmt_qty(&t.qty)));
                                app.metrics.inc_trades();
                                let side_str = match t.side {
                                    hft_core::OrderSide::Buy => "Buy",
                                    hft_core::OrderSide::Sell => "Sell",
                                };
                                app.push_trade(TradeLine {
                                    symbol: t.symbol.clone(),
                                    price: fmt_price(&t.price),
                                    qty: fmt_qty(&t.qty),
                                    side: side_str.to_string(),
                                });
                                let feed_log = format!(
                                    "Feed trade: {} {} qty={} @ {} (market trades: {})",
                                    side_str,
                                    t.symbol,
                                    fmt_qty(&t.qty),
                                    fmt_price(&t.price),
                                    app.metrics.trades_received(),
                                );
                                write_log(app, log_file, &feed_log);
                            }
                            FeedEvent::Ticker(_) => {}
                        }
                    }
                    Err(broadcast::error::TryRecvError::Lagged(n)) => {
                        if n > 0 {
                            let prev = app.metrics.feed_events_lagged();
                            app.metrics.inc_feed_events_lagged(n);
                            let total = app.metrics.feed_events_lagged();
                            // Warn every 100 cumulative dropped events so operator knows consumer is slow.
                            if prev / 100 < total / 100 {
                                tracing::warn!(
                                    dropped = n,
                                    total_lagged = total,
                                    "feed broadcast lagging: consumer too slow, consider reducing strategy work"
                                );
                            }
                            let lag_log = format!(
                                "Feed lagged (dropped {} messages, {} total); resyncing.",
                                n, total
                            );
                            write_log(app, log_file, &lag_log);
                        }
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Closed) => break,
                    Err(broadcast::error::TryRecvError::Empty) => break,
                }
            }
        }

        terminal.draw(|f: &mut Frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(8),
                    Constraint::Min(16),
                    Constraint::Length(8),
                    Constraint::Min(6),
                ])
                .split(f.area());

            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);

            let order_book = order_book_widget(app.order_book.as_ref(), app.book_depth, top[0]);
            f.render_widget(order_book, top[0]);

            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(4), Constraint::Min(4)])
                .split(top[1]);
            let trades = trades_widget(&app.recent_trades, right[0]);
            f.render_widget(trades, right[0]);
            let positions = positions_widget(&app.positions);
            f.render_widget(positions, right[1]);

            let pnl = pnl_latency_widget(app, app.metrics.as_ref());
            f.render_widget(pnl, chunks[1]);

            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[2]);
            let logs = logs_widget(&app.log_lines);
            f.render_widget(logs, bottom[0]);
            let price_feed = price_feed_widget(&app.price_feed_lines);
            f.render_widget(price_feed, bottom[1]);

            let strategy_comp = strategy_comparison_widget(&app.strategy_comparison, chunks[3]);
            f.render_widget(strategy_comp, chunks[3]);
        })?;

        // Poll for input with a short timeout
        let timeout = std::time::Duration::from_millis(50);
        if crossterm::event::poll(timeout)? {
            if let crossterm::event::Event::Key(k) = crossterm::event::read()? {
                if k.kind == crossterm::event::KeyEventKind::Press {
                    match k.code {
                        crossterm::event::KeyCode::Char('q') => break,
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}
