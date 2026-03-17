//! Compute performance metrics from trades or equity curve.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::trade::SimulatedTrade;

/// Aggregated performance report (output of [PerformanceMetrics::compute]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub total_pnl: f64,
    pub win_count: u64,
    pub loss_count: u64,
    pub total_trades: u64,
    pub win_rate: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub avg_trade_duration_secs: f64,
    pub trade_frequency_per_hour: f64,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

impl Default for PerformanceReport {
    fn default() -> Self {
        Self {
            total_pnl: 0.0,
            win_count: 0,
            loss_count: 0,
            total_trades: 0,
            win_rate: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            avg_trade_duration_secs: 0.0,
            trade_frequency_per_hour: 0.0,
            start_time: None,
            end_time: None,
        }
    }
}

/// Computes [PerformanceReport] from a list of [SimulatedTrade]s.
pub struct PerformanceMetrics;

impl PerformanceMetrics {
    /// Risk-free rate per period (e.g. annual). Used for Sharpe. Default 0.0.
    pub const DEFAULT_RISK_FREE_RATE: f64 = 0.0;

    /// Compute full performance report from trades.
    pub fn compute(trades: &[SimulatedTrade], risk_free_rate: f64) -> PerformanceReport {
        if trades.is_empty() {
            return PerformanceReport::default();
        }

        let outcomes: Vec<_> = trades.iter().map(|t| t.to_outcome()).collect();
        let total_pnl: f64 = outcomes.iter().map(|o| o.pnl()).sum();
        let win_count = outcomes.iter().filter(|o| o.pnl() > 0.0).count() as u64;
        let loss_count = outcomes.iter().filter(|o| o.pnl() < 0.0).count() as u64;
        let total_trades = outcomes.len() as u64;
        let win_rate = if total_trades > 0 {
            win_count as f64 / total_trades as f64
        } else {
            0.0
        };

        let (max_drawdown, max_drawdown_pct) = Self::drawdown_from_trades(&outcomes);
        let avg_duration = if total_trades > 0 {
            outcomes.iter().map(|o| o.duration_secs()).sum::<f64>() / total_trades as f64
        } else {
            0.0
        };

        let start_time = trades.first().map(|t| t.entry_time);
        let end_time = trades.last().map(|t| t.exit_time);
        let period_hours = start_time
            .zip(end_time)
            .map(|(s, e)| (e - s).num_seconds() as f64 / 3600.0)
            .unwrap_or(1.0)
            .max(1e-6);
        let trade_frequency_per_hour = total_trades as f64 / period_hours;

        let sharpe_ratio = Self::sharpe_from_trades(&outcomes, risk_free_rate);

        PerformanceReport {
            total_pnl,
            win_count,
            loss_count,
            total_trades,
            win_rate,
            sharpe_ratio,
            max_drawdown,
            max_drawdown_pct,
            avg_trade_duration_secs: avg_duration,
            trade_frequency_per_hour,
            start_time,
            end_time,
        }
    }

    /// Build equity curve (cumulative PnL over time) from outcomes, then compute drawdown.
    fn drawdown_from_trades(outcomes: &[crate::trade::TradeOutcome]) -> (f64, f64) {
        let mut equity = 0.0f64;
        let mut peak = 0.0f64;
        let mut max_dd = 0.0f64;
        let mut max_dd_pct = 0.0f64;
        for o in outcomes {
            equity += o.pnl();
            if equity > peak {
                peak = equity;
            }
            let dd = peak - equity;
            if dd > max_dd {
                max_dd = dd;
            }
            if peak > 1e-20 {
                let pct = 100.0 * (peak - equity) / peak;
                if pct > max_dd_pct {
                    max_dd_pct = pct;
                }
            }
        }
        (max_dd, max_dd_pct)
    }

    /// PnL-based Sharpe proxy (NOT comparable to standard finance Sharpe ratio).
    ///
    /// Returns: (mean_trade_pnl - risk_free_rate/252) / std_dev_trade_pnl * sqrt(252).
    ///
    /// Limitations:
    /// - Numerator is USDT per trade; denominator is also USDT per trade — the ratio is
    ///   dimensionless but not a true return-based Sharpe.
    /// - Annualization factor sqrt(252) assumes one trade per calendar day, which is wrong
    ///   for HFT where hundreds of trades may occur per minute.
    /// - Use this only for relative comparison between strategies, not as an absolute metric.
    fn sharpe_from_trades(
        outcomes: &[crate::trade::TradeOutcome],
        risk_free_rate: f64,
    ) -> f64 {
        if outcomes.len() < 2 {
            return 0.0;
        }
        let returns: Vec<f64> = outcomes.iter().map(|o| o.pnl()).collect();
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>()
            / (returns.len() - 1) as f64;
        let std = variance.sqrt();
        if std < 1e-20 {
            return 0.0;
        }
        let excess = mean - risk_free_rate / 252.0;
        excess / std * (252.0_f64).sqrt()
    }
}
