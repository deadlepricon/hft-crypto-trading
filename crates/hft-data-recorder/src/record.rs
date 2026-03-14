//! Write (timestamp, FeedEvent) to JSONL file.

use chrono::{DateTime, Utc};
use hft_feed_handler::FeedEvent;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Writes recorded events to a JSONL file (one JSON object per line).
pub struct RecordWriter {
    writer: BufWriter<std::fs::File>,
}

#[derive(serde::Serialize)]
struct RecordLine {
    ts: DateTime<Utc>,
    #[serde(rename = "event")]
    event: FeedEvent,
}

impl RecordWriter {
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let f = std::fs::File::create(path)?;
        Ok(Self {
            writer: BufWriter::new(f),
        })
    }

    pub fn write(&mut self, ts: DateTime<Utc>, event: &FeedEvent) -> std::io::Result<()> {
        let line = RecordLine { ts, event: event.clone() };
        serde_json::to_writer(&mut self.writer, &line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}
