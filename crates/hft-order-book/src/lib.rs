//! # hft-order-book
//!
//! In-memory order book maintaining bids and asks for one or more symbols.
//! Designed for low latency: single writer (feed handler) with fast snapshot
//! generation for strategy and UI consumers.

mod book;

pub use book::OrderBook;
