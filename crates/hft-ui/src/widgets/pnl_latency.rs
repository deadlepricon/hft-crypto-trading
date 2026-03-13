//! PnL, win rate, cumulative P&L, and latency metrics widget.

use hft_metrics::Metrics;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

/// Build the PnL and latency info paragraph.
pub fn pnl_latency_widget(app: &App, metrics: &Metrics) -> Paragraph<'static> {
    let win_rate = app.win_rate() * 100.0;
    let lines = vec![
        Line::from(Span::styled(
            " PnL & Metrics\n",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  Cumulative PnL:   {:.2}", app.cumulative_pnl)),
        Line::from(format!("  Win rate:         {:.1}%", win_rate)),
        Line::from(format!("  Wins / Losses:    {} / {}", app.wins, app.losses)),
        Line::from(""),
        Line::from(format!(
            "  Feed latency:     {} µs",
            metrics.latency_feed_us()
        )),
        Line::from(format!("  Feed messages:    {}", metrics.feed_messages())),
    ];
    Paragraph::new(lines)
        .block(Block::default().title(" PnL & Latency ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
