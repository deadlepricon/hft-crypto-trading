//! Persistence layer: append-only log of trades and order events.
//!
//! Starter implementation: write JSON lines to a file. Can be extended to
//! use a proper DB or message log.

use hft_core::events::FillEvent;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::debug;

/// Persistence handle: thread-safe append of events.
pub struct Persist {
    file: Mutex<tokio::fs::File>,
}

impl Persist {
    /// Open or create a log file at the given path.
    pub async fn new(path: impl AsRef<Path>) -> hft_core::Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// Append a fill event as a JSON line.
    pub async fn append_fill(&self, fill: &FillEvent) -> hft_core::Result<()> {
        let line = serde_json::to_string(fill).map_err(|e| hft_core::HftError::Serialization(e.to_string()))?;
        let mut guard = self.file.lock().await;
        guard.write_all(line.as_bytes()).await?;
        guard.write_all(b"\n").await?;
        guard.flush().await?;
        debug!(order_id = %fill.order_id.0, "persisted fill");
        Ok(())
    }
}
