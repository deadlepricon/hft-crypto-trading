//! Cancels panel: shows recent stale-order cancel events, colour-coded by cancel reason.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::CancelLine;

/// Colour for a cancel reason tag.
fn reason_color(reason: &str) -> Color {
    match reason {
        "DRIFT"     => Color::Yellow,
        "INVENTORY" => Color::Cyan,
        "LOSS"      => Color::Red,
        _           => Color::Magenta,
    }
}

pub fn cancels_widget(cancels: &std::collections::VecDeque<CancelLine>) -> Paragraph<'static> {
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        " Recent Cancels\n",
        Style::default().add_modifier(Modifier::BOLD),
    ))];

    for c in cancels.iter().rev().take(20) {
        let side_color = if c.side.to_lowercase().contains("buy") {
            Color::Green
        } else {
            Color::Red
        };
        let reason_col = reason_color(&c.cancel_reason);
        let mut spans = vec![
            Span::styled("\u{2715} ", Style::default().fg(Color::Magenta)),
            Span::raw(format!("[{}] ", c.timestamp)),
            Span::styled(
                format!("[{}] ", c.cancel_reason),
                Style::default().fg(reason_col).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", c.side.to_uppercase()),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("${}", c.original_quote_price),
                Style::default().fg(side_color),
            ),
            Span::raw(format!(" (mid@{})", c.price_at_cancel)),
        ];
        if let Some(ref requote) = c.linked_requote {
            spans.push(Span::raw(" \u{2192} "));
            spans.push(Span::styled(
                format!("REQUOTE {}", requote),
                Style::default().fg(Color::Cyan),
            ));
        }
        lines.push(Line::from(spans));
    }

    if cancels.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No cancels yet.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    Paragraph::new(lines)
        .block(Block::default().title(" Cancels ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
