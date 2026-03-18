//! Positions widget: current positions with entry price and unrealized PnL.

use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::PositionLine;

/// Build the positions paragraph.
pub fn positions_widget(positions: &[PositionLine]) -> Paragraph<'static> {
    let mut lines: Vec<Line> = vec![Line::from("  Symbol    Qty   Entry     Unrealized PnL")];
    for p in positions.iter().take(10) {
        // Truncate qty to 12 chars so an overflow value never wraps the panel layout.
        let qty_display = if p.qty.len() > 12 { "OVERFLOW".to_string() } else { p.qty.clone() };
        lines.push(Line::from(format!(
            "  {:>8}  {:>12}  {:>10}  {}",
            p.symbol, qty_display, p.entry_price, p.unrealized_pnl
        )));
    }
    if positions.is_empty() {
        lines.push(Line::from("  (no positions)"));
    }
    Paragraph::new(lines)
        .block(Block::default().title(" Positions ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
