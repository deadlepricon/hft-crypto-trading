//! # hft-feed-handler
//!
//! Connects to exchange WebSocket APIs via [ExchangeConnector], normalizes
//! incoming market data, maintains the in-memory order book, and broadcasts
//! updates (snapshots/deltas, trades) to the rest of the system.

mod handler;

pub use handler::{FeedEvent, FeedHandler};
