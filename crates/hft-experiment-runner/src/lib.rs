//! Experiment runner: run multiple strategies × parameter configs, store and rank results.

mod record;
mod runner;

pub use record::{ExperimentRecord, ExperimentStore};
pub use runner::ExperimentRunner;
