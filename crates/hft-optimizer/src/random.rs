//! Random search: sample N random points from the parameter space.

use rand::Rng;
use std::collections::HashMap;

use crate::params::{ParameterSpace, ParamValue};

/// Random search over [ParameterSpace]: sample `n_samples` random combinations.
pub struct RandomSearch {
    space: ParameterSpace,
    names: Vec<String>,
    n_samples: usize,
    rng: rand::rngs::StdRng,
}

impl RandomSearch {
    pub fn new(space: ParameterSpace, n_samples: usize, seed: u64) -> Self {
        use rand::SeedableRng;
        let names = space.params.keys().cloned().collect();
        Self {
            space,
            names,
            n_samples,
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }

    pub fn with_names(mut self, names: Vec<String>) -> Self {
        self.names = names;
        if self.names.is_empty() {
            self.names = self.space.params.keys().cloned().collect();
        }
        self
    }

    /// Generate `n_samples` random parameter combinations.
    pub fn sample(mut self) -> Vec<HashMap<String, ParamValue>> {
        let mut out = Vec::with_capacity(self.n_samples);
        for _ in 0..self.n_samples {
            let mut point = HashMap::new();
            for name in &self.names {
                let values = match self.space.params.get(name) {
                    Some(v) if !v.is_empty() => v,
                    _ => continue,
                };
                let i = self.rng.gen_range(0..values.len());
                point.insert(name.clone(), values[i].clone());
            }
            out.push(point);
        }
        out
    }
}
