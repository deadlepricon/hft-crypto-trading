//! Positions widget: current positions with entry price and unrealized PnL.

use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::PositionLine;

/// Build the positions paragraph.
pub fn positions_widget(positions: &[PositionLine]) -> Paragraph<'static> {
    let mut lines: Vec<Line> = vec![Line::from("  Symbol    Qty   Entry     Unrealized PnL")];
    for p in positions.iter().take(10) {
        lines.push(Line::from(format!(
            "  {:>8}  {:>8}  {:>10}  {}",
            p.symbol, p.qty, p.entry_price, p.unrealized_pnl
        )));
    }
    if positions.is_empty() {
        lines.push(Line::from("  (no positions)"));
    }
    Paragraph::new(lines)
        .block(Block::default().title(" Positions ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
