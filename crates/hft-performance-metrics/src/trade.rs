//! Trade representation for metrics computation.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Outcome of a single trade (for PnL and win/loss).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeOutcome {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub side: TradeSide,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub qty: Decimal,
    /// Realized PnL for this trade.
    pub pnl: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

impl TradeOutcome {
    /// Realized PnL for this trade.
    pub fn pnl(&self) -> f64 {
        self.pnl
    }

    pub fn duration_secs(&self) -> f64 {
        (self.exit_time - self.entry_time).num_milliseconds() as f64 / 1000.0
    }
}

/// A simulated or recorded trade (entry + exit); used as input to [PerformanceMetrics].
#[derive(Debug, Clone)]
pub struct SimulatedTrade {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub side: TradeSide,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub qty: Decimal,
}

impl SimulatedTrade {
    pub fn to_outcome(&self) -> TradeOutcome {
        let pnl = Self::pnl_f64(
            self.side,
            self.entry_price,
            self.exit_price,
            self.qty,
        );
        TradeOutcome {
            entry_time: self.entry_time,
            exit_time: self.exit_time,
            side: self.side,
            entry_price: self.entry_price,
            exit_price: self.exit_price,
            qty: self.qty,
            pnl,
        }
    }

    fn pnl_f64(side: TradeSide, entry: Decimal, exit: Decimal, qty: Decimal) -> f64 {
        let e: f64 = entry.try_into().unwrap_or(0.0);
        let x: f64 = exit.try_into().unwrap_or(0.0);
        let q: f64 = qty.try_into().unwrap_or(0.0);
        match side {
            TradeSide::Buy => (x - e) * q,
            TradeSide::Sell => (e - x) * q,
        }
    }
}
