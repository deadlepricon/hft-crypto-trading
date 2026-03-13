//! Recent trades widget.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::collections::VecDeque;

use crate::app::TradeLine;

/// Build the recent trades paragraph.
pub fn trades_widget(trades: &VecDeque<TradeLine>, _area: Rect) -> Paragraph<'static> {
    let mut lines: Vec<Line> = vec![Line::from("  Price      Qty   Side")];
    for t in trades.iter().rev().take(20) {
        let side_style = if t.side.to_lowercase().contains('b') {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {:>10}  {:>8}  ", t.price, t.qty)),
            Span::styled(t.side.clone(), side_style),
        ]));
    }
    Paragraph::new(lines)
        .block(Block::default().title(" Recent Trades ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
