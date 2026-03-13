//! TUI event loop and render.

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;
use ratatui::Terminal;
use std::io;
use std::time::Duration;

use crate::app::App;
use crate::widgets::{
    logs_widget, order_book_widget, pnl_latency_widget, positions_widget, trades_widget,
};

/// Run the TUI until the user quits (e.g. 'q').
pub fn run_ui(mut app: App) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f: &mut Frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10),
                    Constraint::Length(12),
                ])
                .split(f.area());

            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(chunks[0]);

            let left = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(top[0]);

            let order_book = order_book_widget(
                &app.order_book,
                app.book_depth,
                left[0],
            );
            f.render_widget(order_book, left[0]);

            let trades = trades_widget(&app.recent_trades, left[1]);
            f.render_widget(trades, left[1]);

            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(10),
                    Constraint::Length(12),
                    Constraint::Min(5),
                ])
                .split(top[1]);

            let pnl = pnl_latency_widget(&app, &app.metrics);
            f.render_widget(pnl, right[0]);

            let positions = positions_widget(&app.positions);
            f.render_widget(positions, right[1]);

            let logs = logs_widget(&app.log_lines);
            f.render_widget(logs, right[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        _ => {}
                    }
                }
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}
