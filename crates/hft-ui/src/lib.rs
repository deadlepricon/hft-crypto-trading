//! # hft-ui
//!
//! Terminal UI (TUI) built with ratatui. Displays live order book, recent trades,
//! price charts, positions, PnL, win rate, cumulative P&L, latency metrics,
//! and system logs in a professional, information-dense layout.

mod app;
mod run;
pub mod widgets;

pub use app::App;
pub use run::run_ui;
pub use hft_execution::CancelEvent;
