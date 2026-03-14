//! Strategy comparison widget: PnL, win rate, drawdown, best parameter sets.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::StrategyComparisonLine;

/// Build the strategy comparison table paragraph.
pub fn strategy_comparison_widget(rows: &[StrategyComparisonLine], _area: Rect) -> Paragraph<'static> {
    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(
            " Strategy Comparison (Backtest / Optimization)\n",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])];
    lines.push(Line::from(format!(
        "  {:<20} {:>12} {:>8} {:>10} {:>8}  Best params",
        "Strategy", "PnL", "Win%", "Drawdown%", "Sharpe"
    )));
    lines.push(Line::from("  ".to_string() + &"-".repeat(90)));
    for r in rows.iter().take(15) {
        lines.push(Line::from(format!(
            "  {:<20} {:>12.4} {:>7.1}% {:>9.1}% {:>8.2}  {}",
            truncate(&r.strategy_name, 18),
            r.pnl,
            r.win_rate_pct,
            r.max_drawdown_pct,
            r.sharpe,
            truncate(&r.best_params, 30)
        )));
    }
    if rows.is_empty() {
        lines.push(Line::from("  (no experiment results loaded)"));
    }
    Paragraph::new(lines)
        .block(Block::default().title(" Strategy Comparison ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
