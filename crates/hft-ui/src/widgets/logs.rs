//! System logs widget.

use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::collections::VecDeque;

/// Build the log viewer paragraph.
pub fn logs_widget(log_lines: &VecDeque<String>) -> Paragraph<'static> {
    let lines: Vec<Line> = log_lines
        .iter()
        .rev()
        .take(30)
        .map(|s| Line::from(s.clone()))
        .collect();
    Paragraph::new(lines)
        .block(Block::default().title(" System Logs ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
