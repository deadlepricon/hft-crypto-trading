//! Experiment record and storage for reproducibility.

use chrono::{DateTime, Utc};
use hft_performance_metrics::PerformanceReport;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use hft_optimizer::ParamValue;

/// Single experiment record: strategy name, params, metrics, timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRecord {
    pub run_id: String,
    pub strategy_name: String,
    pub params: HashMap<String, ParamValue>,
    pub metrics: PerformanceReport,
    pub timestamp: DateTime<Utc>,
}

impl ExperimentRecord {
    pub fn new(
        run_id: impl Into<String>,
        strategy_name: impl Into<String>,
        params: HashMap<String, ParamValue>,
        metrics: PerformanceReport,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            strategy_name: strategy_name.into(),
            params,
            metrics,
            timestamp: Utc::now(),
        }
    }
}

/// Serialize [ParamValue] for JSON (serde_json doesn't handle ParamValue by default without impl).
/// We use a simple string representation for storage.
pub fn params_to_json(params: &HashMap<String, ParamValue>) -> Result<String, serde_json::Error> {
    let map: HashMap<String, String> = params
        .iter()
        .map(|(k, v): (&String, &ParamValue)| {
            let s = match v {
                ParamValue::Float(f) => f.to_string(),
                ParamValue::Int(i) => i.to_string(),
                ParamValue::Bool(b) => b.to_string(),
            };
            (k.clone(), s)
        })
        .collect();
    serde_json::to_string(&map)
}

/// Store experiment records (e.g. append to JSONL file).
pub trait ExperimentStore: Send + Sync {
    fn save(&self, record: &ExperimentRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// In-memory store for tests or small runs.
#[derive(Default)]
pub struct MemoryStore {
    pub records: std::sync::RwLock<Vec<ExperimentRecord>>,
}

impl ExperimentStore for MemoryStore {
    fn save(&self, record: &ExperimentRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.records.write().unwrap().push(record.clone());
        Ok(())
    }
}

/// Append experiment records to a JSONL file (one JSON object per line).
pub struct JsonlFileStore {
    path: std::path::PathBuf,
}

impl JsonlFileStore {
    pub fn new(path: impl AsRef<std::path::Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl ExperimentStore for JsonlFileStore {
    fn save(&self, record: &ExperimentRecord) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let line = serde_json::to_string(record)?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        use std::io::Write;
        writeln!(f, "{}", line)?;
        Ok(())
    }
}
