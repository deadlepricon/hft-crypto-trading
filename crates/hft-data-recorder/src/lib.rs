//! Record market data (FeedEvent) to file and replay for backtesting.
//!
//! Record: write (timestamp, FeedEvent) as JSONL. Replay: read and yield [ReplayEvent] for [BacktestRunner].

mod record;
mod replay;

pub use record::RecordWriter;
pub use replay::ReplayReader;
