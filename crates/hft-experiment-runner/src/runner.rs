//! Run multiple experiments (strategies × params), store and rank.

use hft_optimizer::{OptimizationResult, OptimizationRunner};

use crate::record::{ExperimentRecord, ExperimentStore};

/// Runs multiple backtests (e.g. from grid or random search), saves each as [ExperimentRecord],
/// and returns results ranked by a chosen metric.
pub struct ExperimentRunner<S> {
    store: S,
}

impl<S: ExperimentStore> ExperimentRunner<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Save a single optimization result as an experiment record.
    pub fn save_result(
        &self,
        strategy_name: impl Into<String>,
        result: OptimizationResult,
        run_id: impl Into<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let record = ExperimentRecord::new(
            run_id.into(),
            strategy_name,
            result.params,
            result.report,
        );
        self.store.save(&record)
    }

    /// Rank results by Sharpe (descending) and return top N.
    pub fn rank_by_sharpe(results: Vec<OptimizationResult>, top_n: usize) -> Vec<OptimizationResult> {
        let mut sorted = OptimizationRunner::rank_by_sharpe(results);
        sorted.truncate(top_n);
        sorted
    }

    /// Rank results by total PnL (descending) and return top N.
    pub fn rank_by_pnl(results: Vec<OptimizationResult>, top_n: usize) -> Vec<OptimizationResult> {
        let mut sorted = OptimizationRunner::rank_by_pnl(results);
        sorted.truncate(top_n);
        sorted
    }
}
