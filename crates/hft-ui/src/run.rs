//! TUI event loop and layout: run the terminal UI with order book, trades, PnL, logs.

use hft_core::events::EventEnvelope;
use hft_execution::PaperFill;
use hft_feed_handler::FeedEvent;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;
use ratatui::Terminal;
use std::io::Stdout;
use tokio::sync::{broadcast, mpsc};

use crate::app::{App, TradeLine};
use crate::widgets::{
    logs_widget, order_book_widget, pnl_latency_widget, positions_widget, price_feed_widget,
    strategy_comparison_widget, trades_widget,
};

/// Run the TUI. If `feed_rx` is Some, drain feed events for price feed and market trades.
/// If `fill_rx` is Some (paper trading), drain our fills and update PnL / recent trades.
/// Exit with 'q' or Ctrl+C.
pub fn run_ui(
    mut app: App,
    feed_rx: Option<broadcast::Receiver<EventEnvelope<FeedEvent>>>,
    fill_rx: Option<mpsc::UnboundedReceiver<PaperFill>>,
) -> Result<(), Box<dyn std::error::Error>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_loop(&mut terminal, &mut app, feed_rx, fill_rx);

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    mut feed_rx: Option<broadcast::Receiver<EventEnvelope<FeedEvent>>>,
    mut fill_rx: Option<mpsc::UnboundedReceiver<PaperFill>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Drain paper fills first so PnL / Our fills update before we draw
        if let Some(rx) = fill_rx.as_mut() {
            while let Ok(fill) = rx.try_recv() {
                app.metrics.inc_fills();
                app.record_trade_result(fill.pnl_delta, fill.is_buy, fill.unrealized_pnl);
                let side_str = if fill.is_buy { "Buy" } else { "Sell" };
                app.push_trade(TradeLine {
                    symbol: fill.request.symbol.clone(),
                    price: fill.fill_price.to_string(),
                    qty: fill.request.qty.to_string(),
                    side: side_str.to_string(),
                });
            }
        }
        // Drain feed events (price feed + market trades)
        if let Some(rx) = feed_rx.as_mut() {
            while let Ok(env) = rx.try_recv() {
                match &env.payload {
                    FeedEvent::OrderBookSnapshot(_) | FeedEvent::OrderBookDelta(_) => {
                        let best_bid = app.order_book.best_bid().map(|p| p.to_string()).unwrap_or_default();
                        let best_ask = app.order_book.best_ask().map(|p| p.to_string()).unwrap_or_default();
                        if !best_bid.is_empty() || !best_ask.is_empty() {
                            app.push_price_feed(format!("book best_bid={} best_ask={}", best_bid, best_ask));
                        }
                    }
                    FeedEvent::Trade(t) => {
                        app.push_price_feed(format!("trade {:?} @ {} qty={}", t.side, t.price, t.qty));
                        app.metrics.inc_trades();
                        let side_str = match t.side {
                            hft_core::OrderSide::Buy => "Buy",
                            hft_core::OrderSide::Sell => "Sell",
                        };
                        app.push_trade(TradeLine {
                            symbol: t.symbol.clone(),
                            price: t.price.to_string(),
                            qty: t.qty.to_string(),
                            side: side_str.to_string(),
                        });
                    }
                    FeedEvent::Ticker(_) => {}
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
