//! Parameter space definition for strategy tuning.

use serde::{Deserialize, Serialize};

/// A single parameter value (discrete or continuous).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamValue {
    Float(f64),
    Int(i64),
    Bool(bool),
}

/// Named parameter space: each key is a parameter name, values are the set to search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParameterSpace {
    pub params: std::collections::HashMap<String, Vec<ParamValue>>,
}

impl ParameterSpace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_float(mut self, name: impl Into<String>, values: Vec<f64>) -> Self {
        self.params
            .insert(name.into(), values.into_iter().map(ParamValue::Float).collect());
        self
    }

    pub fn add_int(mut self, name: impl Into<String>, values: Vec<i64>) -> Self {
        self.params
            .insert(name.into(), values.into_iter().map(ParamValue::Int).collect());
        self
    }

    /// Number of points in the full grid (product of param lengths).
    pub fn grid_len(&self) -> usize {
        self.params
            .values()
            .map(|v| v.len().max(1))
            .product()
    }
}
