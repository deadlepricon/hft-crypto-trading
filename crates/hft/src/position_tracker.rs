//! Tracks position and average entry for PnL on paper fills.
//! Uses f64 for position size; clamps inputs and outputs to avoid Inf/NaN and crazy display values.

use hft_execution::PositionTracker;
use hft_core::{OrderRequest, OrderSide};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Clamp PnL values to finite and reasonable range so UI never shows 2e18.
const PNL_CLAMP: f64 = 1e10;
/// Max realized PnL from a single fill (avoids one bad fill blowing up the display).
const PNL_DELTA_MAX_PER_FILL: f64 = 1e6;
/// Reject order qty outside this range (e.g. 1e-9 to 1e6 BTC) to avoid garbage positions.
const QTY_MIN: f64 = 1e-9;
const QTY_MAX: f64 = 1e6;
/// Positions smaller than this are treated as flat (avoids weighted-avg blow-up when dividing by tiny qty).
const POSITION_ZERO_THRESHOLD: f64 = 1e-6;

struct PositionState {
    /// Position size: positive = long, negative = short (in asset units, e.g. BTC).
    qty: f64,
    entry_price: f64,
}

/// Single-symbol position tracker for paper PnL.
pub struct PaperPositionTracker {
    positions: RwLock<HashMap<String, PositionState>>,
}

impl PaperPositionTracker {
    pub fn new() -> Self {
        Self {
            positions: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for PaperPositionTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PositionTracker for PaperPositionTracker {
    fn apply_fill(&self, req: &OrderRequest, fill_price: Decimal) -> (f64, bool, f64) {
        let is_buy = matches!(req.side, OrderSide::Buy);
        let price_f: f64 = fill_price.to_string().parse().unwrap_or(0.0);

        let mut guard = self.positions.write();
        let pos = guard.entry(req.symbol.clone()).or_insert(PositionState {
            qty: 0.0,
            entry_price: 0.0,
        });

        let qty_f: f64 = req.qty.to_string().parse().unwrap_or(0.0);
        let qty_f = qty_f.clamp(QTY_MIN, QTY_MAX);
        let signed_qty_f = if is_buy { qty_f } else { -qty_f };

        let mut pnl_delta = 0.0;
        if signed_qty_f > 0.0 {
            // Buy: add to long (or reduce/close short)
            if pos.qty < -1e-12 {
                // We're short: part or all of this buy closes the short → realize PnL
                let close_qty = signed_qty_f.min(-pos.qty);
                if close_qty > 0.0 {
                    pnl_delta = (pos.entry_price - price_f) * close_qty; // short profit when buy back lower
                }
            }
            let new_qty = pos.qty + signed_qty_f;
            if new_qty.abs() < POSITION_ZERO_THRESHOLD {
                pos.qty = 0.0;
                pos.entry_price = 0.0;
            } else {
                pos.entry_price = if pos.qty == 0.0 {
                    price_f
                } else {
                    let wavg = (pos.entry_price * pos.qty + price_f * signed_qty_f) / new_qty;
                    if wavg.is_finite() && wavg.abs() < 1e12 {
                        wavg
                    } else {
                        price_f
                    }
                };
                pos.qty = new_qty;
            }
        } else {
            // Sell: close long and/or open/add to short
            let close_qty = (-signed_qty_f).min(pos.qty);
            if close_qty > 0.0 {
                pnl_delta = (price_f - pos.entry_price) * close_qty;
                pos.qty -= close_qty;
            }
            // Remaining sold qty opens or adds to short
            let short_qty = -signed_qty_f - close_qty;
            if short_qty > 0.0 {
                let prev_qty = pos.qty;
                pos.qty -= short_qty; // now negative (short)
                if prev_qty >= 0.0 {
                    pos.entry_price = price_f;
                } else {
                    let old_short = -prev_qty;
                    let new_short = -pos.qty;
                    if new_short >= POSITION_ZERO_THRESHOLD {
                        let wavg = (pos.entry_price * old_short + price_f * short_qty) / new_short;
                        if wavg.is_finite() && wavg.abs() < 1e12 {
                            pos.entry_price = wavg;
                        } else {
                            pos.entry_price = price_f;
                        }
                    } else {
                        pos.entry_price = price_f;
                    }
                }
            }
            if pos.qty.abs() < POSITION_ZERO_THRESHOLD {
                pos.qty = 0.0;
                pos.entry_price = 0.0;
            }
        }

        // Unrealized PnL: position * (mark - entry). Long profits when mark > entry; short when mark < entry.
        let mut unrealized = pos.qty * (price_f - pos.entry_price);
        if !unrealized.is_finite() || unrealized.abs() > PNL_CLAMP {
            unrealized = 0.0;
        } else {
            unrealized = unrealized.clamp(-PNL_CLAMP, PNL_CLAMP);
        }
        if !pnl_delta.is_finite() || pnl_delta.abs() > PNL_CLAMP {
            pnl_delta = 0.0;
        } else {
            pnl_delta = pnl_delta
                .clamp(-PNL_DELTA_MAX_PER_FILL, PNL_DELTA_MAX_PER_FILL)
                .clamp(-PNL_CLAMP, PNL_CLAMP);
        }

        (pnl_delta, is_buy, unrealized)
    }
}
