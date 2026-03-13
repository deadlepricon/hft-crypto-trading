//! TUI widgets: order book, trades, positions, PnL, latency, logs, etc.

mod order_book;
mod trades;
mod positions;
mod pnl_latency;
mod logs;

pub use order_book::order_book_widget;
pub use trades::trades_widget;
pub use positions::positions_widget;
pub use pnl_latency::pnl_latency_widget;
pub use logs::logs_widget;
