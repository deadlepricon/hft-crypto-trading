//! TUI widgets: order book, trades, positions, PnL, latency, logs, price feed.

mod cancels;
mod order_book;
mod trades;
mod positions;
mod pnl_latency;
mod logs;
mod price_feed;
mod strategy_comparison;

pub use cancels::cancels_widget;
pub use order_book::order_book_widget;
pub use trades::trades_widget;
pub use positions::positions_widget;
pub use pnl_latency::pnl_latency_widget;
pub use logs::logs_widget;
pub use price_feed::price_feed_widget;
pub use strategy_comparison::strategy_comparison_widget;
