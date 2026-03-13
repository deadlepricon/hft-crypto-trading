//! Order book widget: bids and asks with depth.

use hft_order_book::OrderBook;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Build the order book paragraph for the given area.
pub fn order_book_widget(book: &OrderBook, depth: usize, _area: Rect) -> Paragraph<'static> {
    let (bids, asks, _seq) = book.snapshot(depth);
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Order Book\n", Style::default().add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("  BID        QTY  |  ASK        QTY"));
    lines.push(Line::from("────────────────────────────────────"));
    let n = bids.len().max(asks.len());
    for i in 0..n {
        let bid_str = bids
            .get(i)
            .map(|l| format!("{:>10}  {:>8}", l.price, l.qty))
            .unwrap_or_else(|| "            ".to_string());
        let ask_str = asks
            .get(i)
            .map(|l| format!("{:>10}  {:>8}", l.price, l.qty))
            .unwrap_or_else(|| "            ".to_string());
        let line = if i < bids.len() && i < asks.len() {
            Line::from(vec![
                Span::styled(bid_str, Style::default().fg(Color::Green)),
                Span::raw(" | "),
                Span::styled(ask_str, Style::default().fg(Color::Red)),
            ])
        } else if i < bids.len() {
            Line::from(Span::styled(bid_str, Style::default().fg(Color::Green)))
        } else {
            Line::from(Span::styled(ask_str, Style::default().fg(Color::Red)))
        };
        lines.push(line);
    }
    Paragraph::new(lines)
        .block(Block::default().title(" Order Book ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
