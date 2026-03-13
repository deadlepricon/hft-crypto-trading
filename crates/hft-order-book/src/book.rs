//! Order book implementation.
//!
//! Maintains price-time priority (or price priority) levels. The feed handler
//! applies incremental updates; readers take snapshots without blocking the writer
//! for long. Uses parking_lot RwLock for good read concurrency when taking
//! snapshots (single writer is the feed handler).

use std::collections::BTreeMap;

use hft_core::{Level, Price, Qty, Symbol};
use parking_lot::RwLock;
use rust_decimal::Decimal;

#[derive(Debug, Default)]
struct OrderBookInner {
    bids: BTreeMap<Price, Qty>,
    asks: BTreeMap<Price, Qty>,
    sequence: u64,
}

/// In-memory order book for a single symbol.
/// Bids: descending by price. Asks: ascending by price.
#[derive(Debug)]
pub struct OrderBook {
    symbol: Symbol,
    inner: RwLock<OrderBookInner>,
}

impl OrderBook {
    /// Create a new order book for the given symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            inner: RwLock::new(OrderBookInner::default()),
        }
    }

    /// Apply incremental update: set bid levels (replace or remove with 0 qty).
    pub fn update_bids(&self, levels: &[(Price, Qty)]) {
        let mut guard = self.inner.write();
        for (price, qty) in levels {
            if qty.is_zero() {
                guard.bids.remove(price);
            } else {
                guard.bids.insert(*price, *qty);
            }
        }
        guard.sequence = guard.sequence.wrapping_add(1);
    }

    /// Apply incremental update: set ask levels.
    pub fn update_asks(&self, levels: &[(Price, Qty)]) {
        let mut guard = self.inner.write();
        for (price, qty) in levels {
            if qty.is_zero() {
                guard.asks.remove(price);
            } else {
                guard.asks.insert(*price, *qty);
            }
        }
        guard.sequence = guard.sequence.wrapping_add(1);
    }

    /// Replace entire book (e.g. from a snapshot). Used when reconnecting or initial load.
    pub fn replace(&self, bids: Vec<Level>, asks: Vec<Level>) {
        let mut guard = self.inner.write();
        guard.bids.clear();
        guard.asks.clear();
        for l in bids {
            if !l.qty.is_zero() {
                guard.bids.insert(l.price, l.qty);
            }
        }
        for l in asks {
            if !l.qty.is_zero() {
                guard.asks.insert(l.price, l.qty);
            }
        }
        guard.sequence = guard.sequence.wrapping_add(1);
    }

    /// Take a snapshot of the top `depth` levels on each side.
    pub fn snapshot(&self, depth: usize) -> (Vec<Level>, Vec<Level>, u64) {
        let guard = self.inner.read();
        let bids: Vec<Level> = guard
            .bids
            .iter()
            .rev()
            .take(depth)
            .map(|(p, q)| Level {
                price: *p,
                qty: *q,
            })
            .collect();
        let asks: Vec<Level> = guard
            .asks
            .iter()
            .take(depth)
            .map(|(p, q)| Level {
                price: *p,
                qty: *q,
            })
            .collect();
        (bids, asks, guard.sequence)
    }

    /// Best bid price, if any.
    pub fn best_bid(&self) -> Option<Price> {
        let guard = self.inner.read();
        guard.bids.iter().next_back().map(|(p, _)| *p)
    }

    /// Best ask price, if any.
    pub fn best_ask(&self) -> Option<Price> {
        let guard = self.inner.read();
        guard.asks.iter().next().map(|(p, _)| *p)
    }

    /// Mid price from best bid/ask, if both exist.
    pub fn mid_price(&self) -> Option<Price> {
        let (b, a) = (self.best_bid(), self.best_ask());
        match (b, a) {
            (Some(b), Some(a)) => Some((b + a) / Decimal::from(2)),
            _ => None,
        }
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }
}
