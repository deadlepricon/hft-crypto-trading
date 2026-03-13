//! # hft-metrics
//!
//! Latency and throughput metrics: feed-to-strategy latency, order round-trip,
//! message counts. Uses atomics and lock-free structures for minimal overhead
//! in the hot path.

mod metrics;

pub use metrics::Metrics;
