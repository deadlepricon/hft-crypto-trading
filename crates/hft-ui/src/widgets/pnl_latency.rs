//! PnL, win rate, Sharpe, drawdown, profit per trade/minute, and latency metrics widget.

use hft_metrics::Metrics;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;

/// Format PnL for display: finite and capped so we never show crazy spikes.
fn fmt_pnl(x: f64) -> String {
    if !x.is_finite() || x.abs() >= 1e7 {
        "N/A".to_string()
    } else {
        format!("{:.4}", x)
    }
}

/// Build the PnL and latency info paragraph.
pub fn pnl_latency_widget(app: &App, metrics: &Metrics) -> Paragraph<'static> {
    let win_pct = app.win_rate() * 100.0;
    let buy_win_pct = app.buy_win_rate() * 100.0;
    let sell_win_pct = app.sell_win_rate() * 100.0;
    let total_trades = app.wins + app.losses;
    let live_pnl = app.cumulative_pnl + app.unrealized_pnl;
    let net_fill_rate = app.net_fill_rate() * 100.0;
    let cancel_rate = if app.total_fills + app.total_cancels > 0 {
        app.total_cancels as f64 / (app.total_fills + app.total_cancels) as f64 * 100.0
    } else {
        0.0
    };
    let lines = vec![
        Line::from(Span::styled(
            " PnL & Performance\n",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "  Live PnL (paper):   {}  (realized: {}  unrealized: {})",
            fmt_pnl(live_pnl),
            fmt_pnl(app.cumulative_pnl),
            fmt_pnl(app.unrealized_pnl)
        )),
        Line::from(format!(
            "  Our fills:         {}  (orders we sent that filled; round-trips closed: {})",
            app.total_fills, total_trades
        )),
        Line::from(format!("  Profit per trade:  {}  (on closed only)", fmt_pnl(app.profit_per_trade()))),
        Line::from(""),
        Line::from(format!("  Win rate:          {:.1}%  ({}/{} closed trades)", win_pct, app.wins, total_trades)),
        Line::from(format!("  Buy Win %:         {:.1}%  ({}/{})", buy_win_pct, app.buy_wins, app.buy_wins + app.buy_losses)),
        Line::from(format!("  Sell Win %:        {:.1}%  ({}/{})", sell_win_pct, app.sell_wins, app.sell_wins + app.sell_losses)),
        Line::from(""),
        Line::from(format!("  PnL Sharpe:        {:.2}  (PnL-based proxy, not standard Sharpe)", app.sharpe_ratio())),
        Line::from(format!("  Max drawdown:      {}", fmt_pnl(app.max_drawdown))),
        Line::from(format!("  Profit per minute: {}", fmt_pnl(app.profit_per_minute()))),
        Line::from(""),
        Line::from(format!(
            "  Market trades (feed): {}   Our fills: {}  (our orders filled)",
            metrics.trades_received(),
            metrics.fills()
        )),
        Line::from(format!(
            "  Net fill rate:     {:.1}%  (fills / fills+cancels)   Cancel rate: {:.1}%",
            net_fill_rate,
            cancel_rate,
        )),
        Line::from(format!(
            "  Total Cancels:     {}  ({:.1}/min)   [DRIFT / INVENTORY / LOSS]",
            app.total_cancels,
            app.cancels_per_minute()
        )),
        Line::from(format!(
            "  Feed latency:      {} µs   Feed messages: {}",
            metrics.latency_feed_us(),
            metrics.feed_messages()
        )),
    ];
    Paragraph::new(lines)
        .block(Block::default().title(" PnL & Performance ").borders(Borders::ALL))
        .wrap(Wrap { trim: false })
}
