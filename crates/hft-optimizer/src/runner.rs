//! Optimization runner: run backtests for each parameter set and rank results.

use hft_backtesting::{BacktestConfig, BacktestResult, ReplayEvent};
use hft_performance_metrics::PerformanceReport;
use std::collections::HashMap;
use std::sync::Arc;

use crate::params::ParamValue;

/// Single result: parameter set + backtest performance.
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub params: HashMap<String, ParamValue>,
    pub report: PerformanceReport,
    pub backtest_result: BacktestResult,
}

impl OptimizationResult {
    pub fn sharpe(&self) -> f64 {
        self.report.sharpe_ratio
    }

    pub fn total_pnl(&self) -> f64 {
        self.report.total_pnl
    }

    pub fn win_rate(&self) -> f64 {
        self.report.win_rate
    }
}

/// Runs backtests for each parameter combination and returns ranked results.
pub struct OptimizationRunner;

impl OptimizationRunner {
    /// Run backtest for one parameter set. Strategies must be built from `params`
    /// by the caller (e.g. strategy factory). This helper only runs one backtest
    /// and returns the result.
    pub async fn run_one(
        config: BacktestConfig,
        events: impl IntoIterator<Item = ReplayEvent>,
        strategies: &[Arc<dyn hft_strategy::Strategy>],
    ) -> OptimizationResult {
        let order_book = Arc::new(hft_order_book::OrderBook::new(config.symbol.clone()));
        let runner = hft_backtesting::BacktestRunner::new(config, order_book);
        let result = runner.run(events, strategies).await;
        OptimizationResult {
            params: HashMap::new(),
            report: result.performance_report().clone(),
            backtest_result: result,
        }
    }

    /// Rank results by a metric (e.g. Sharpe, PnL). Returns sorted by `metric` descending.
    pub fn rank_by_sharpe(mut results: Vec<OptimizationResult>) -> Vec<OptimizationResult> {
        results.sort_by(|a, b| b.report.sharpe_ratio.partial_cmp(&a.report.sharpe_ratio).unwrap());
        results
    }

    pub fn rank_by_pnl(mut results: Vec<OptimizationResult>) -> Vec<OptimizationResult> {
        results.sort_by(|a, b| b.report.total_pnl.partial_cmp(&a.report.total_pnl).unwrap());
        results
    }
}
