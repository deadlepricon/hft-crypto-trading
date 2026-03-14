//! Read recorded JSONL and yield ReplayEvent for the backtester.

use chrono::{DateTime, Utc};
use hft_backtesting::ReplayEvent;
use hft_feed_handler::FeedEvent;
use std::path::Path;

#[derive(serde::Deserialize)]
struct RecordLine {
    ts: DateTime<Utc>,
    event: FeedEvent,
}

/// Reads a JSONL file and yields [ReplayEvent]s for [hft_backtesting::BacktestRunner].
pub struct ReplayReader {
    reader: std::io::BufReader<std::fs::File>,
}

impl ReplayReader {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let f = std::fs::File::open(path)?;
        Ok(Self {
            reader: std::io::BufReader::new(f),
        })
    }

    pub fn into_events(self) -> impl Iterator<Item = Result<ReplayEvent, String>> {
        let reader = self.reader;
        std::io::BufRead::lines(reader).map(|line: Result<String, _>| {
            let line = line.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                return Err("empty line".to_string());
            }
            let rec: RecordLine = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            Ok(ReplayEvent {
                ts: rec.ts,
                event: rec.event,
            })
        })
    }
}
