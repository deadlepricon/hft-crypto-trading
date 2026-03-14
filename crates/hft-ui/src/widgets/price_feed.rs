//! Price feed widget: shows all prices coming in (book best bid/ask + trade prices).

use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::collections::VecDeque;

/// Build the price feed paragraph (last N price updates from sim).
pub fn price_feed_widget(lines: &VecDeque<String>) -> Paragraph<'static> {
    let content: Vec<Line> = lines
        .iter()
        .rev()
        .take(20)
        .map(|s| Line::from(s.clone()))
        .collect();
    Paragraph::new(content)
        .block(Block::default().title(" Price feed (incoming) ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
