//! Order book widget: bids and asks with depth.

use hft_order_book::OrderBook;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Build the order book paragraph for the given area.
pub fn order_book_widget(book: &OrderBook, depth: usize, area: Rect) -> Paragraph<'static> {
    // Inner height after borders; cap data rows so we never overflow.
    let inner_h = (area.height as usize).saturating_sub(2);
    // 2 header lines: column labels + separator
    let max_data = inner_h.saturating_sub(2);
    let depth = depth.min(max_data.max(1));

    let (bids, asks, _seq) = book.snapshot(depth);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("BID", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("              "),
        Span::styled("ASK", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from("  Price       Qty    |  Price       Qty"));

    let n = bids.len().max(asks.len());

    if n == 0 {
        let bid = book.best_bid().map(|p| format!("{:.2}", p)).unwrap_or_else(|| "-".into());
        let ask = book.best_ask().map(|p| format!("{:.2}", p)).unwrap_or_else(|| "-".into());
        if bid == "-" && ask == "-" {
            lines.push(Line::from(Span::styled(
                "  Waiting for feed data...",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(format!("  bid={bid}  ask={ask}  (depth loading)")));
        }
    } else {
        for i in 0..n {
            let bid_str = bids
                .get(i)
                .map(|l| format!("{:>10.2}  {:>6.4}", l.price, l.qty))
                .unwrap_or_else(|| "                  ".to_string());
            let ask_str = asks
                .get(i)
                .map(|l| format!("{:>10.2}  {:>6.4}", l.price, l.qty))
                .unwrap_or_else(|| "                  ".to_string());
            let line = if i < bids.len() && i < asks.len() {
                Line::from(vec![
                    Span::styled(bid_str, Style::default().fg(Color::Green)),
                    Span::raw(" | "),
                    Span::styled(ask_str, Style::default().fg(Color::Red)),
                ])
            } else if i < bids.len() {
                Line::from(Span::styled(bid_str, Style::default().fg(Color::Green)))
            } else {
                Line::from(vec![
                    Span::raw("                    | "),
                    Span::styled(ask_str, Style::default().fg(Color::Red)),
                ])
            };
            lines.push(line);
        }
    }

    Paragraph::new(lines)
        .block(Block::default().title(" Order Book ").borders(Borders::ALL))
}
