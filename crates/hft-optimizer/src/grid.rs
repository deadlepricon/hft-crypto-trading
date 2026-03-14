//! Grid search: full Cartesian product of parameter values.

use crate::params::{ParameterSpace, ParamValue};

/// Iterator over all combinations of [ParameterSpace] (grid search).
pub struct GridSearch {
    space: ParameterSpace,
    names: Vec<String>,
    lengths: Vec<usize>,
    n: usize,
    index: usize,
}

impl GridSearch {
    pub fn new(space: ParameterSpace) -> Self {
        let names: Vec<String> = space.params.keys().cloned().collect();
        let lengths: Vec<usize> = names.iter().map(|n| space.params[n].len()).collect();
        let n = lengths.iter().product();
        Self {
            space,
            names,
            lengths,
            n,
            index: 0,
        }
    }

    /// Get the i-th combination as a map (param name -> value).
    pub fn at(&self, i: usize) -> Option<std::collections::HashMap<String, ParamValue>> {
        if i >= self.n {
            return None;
        }
        let mut out = std::collections::HashMap::new();
        let mut idx = i;
        for (name, len) in self.names.iter().zip(self.lengths.iter()) {
            let r = idx % len;
            idx /= len;
            let v = self.space.params[name].get(r)?.clone();
            out.insert(name.clone(), v);
        }
        Some(out)
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

impl Iterator for GridSearch {
    type Item = std::collections::HashMap<String, ParamValue>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.n {
            return None;
        }
        let out = self.at(self.index);
        self.index += 1;
        out
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.n.saturating_sub(self.index);
        (rem, Some(rem))
    }
}
