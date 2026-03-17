//! Market maker strategy: post bid/ask around mid, skew by imbalance and inventory.
//!
//! **Quoting**
//! - Post a buy limit just below mid and a sell limit just above mid (spread_bps).
//! - Use order book imbalance to skew: bid-heavy → shift both quotes up (more aggressive sell);
//!   ask-heavy → shift both quotes down (more aggressive buy).
//!
//! **Inventory**
//! - Track position via [Strategy::on_fill] (fill feedback from execution). Too long: widen ask,
//!   tighten bid to encourage selling. Too short: opposite.
//!
//! **Re-quote only when**
//! - Mid price moves by more than min_tick_move.
//! - Imbalance regime changes (bid-heavy / ask-heavy / neutral).
//! - Inventory skew crosses a threshold (vs last quoted level).

use hft_core::{OrderSide, Price};
use hft_feed_handler::FeedEvent;
use hft_order_book::OrderBook;
use rust_decimal::Decimal;
use std::sync::{Arc, Mutex};
use tracing::debug;

use super::{order_request, Signal, Strategy, StrategyFill};

/// Imbalance regime for re-quote condition.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImbalanceRegime {
    BidHeavy,
    AskHeavy,
    Neutral,
}

/// Configurable parameters for the market maker strategy.
#[derive(Debug, Clone)]
pub struct MarketMakerParams {
    /// Symbol to quote.
    pub symbol: String,
    /// Quantity per limit order.
    pub qty_per_order: Decimal,
    /// Spread in basis points (e.g. 10 = 0.1%).
    pub spread_bps: u64,
    /// Max absolute inventory before skew is capped (same units as qty).
    pub max_inventory: Decimal,
    /// How much to shift quotes from imbalance; in bps per unit imbalance ratio (e.g. 5).
    pub imbalance_skew_factor: Decimal,
    /// Number of top levels to sum for imbalance.
    pub book_depth: usize,
    /// Minimum mid move (in price units) to trigger re-quote.
    pub min_tick_move: Decimal,
    /// Minimum milliseconds between re-quotes; avoids spamming when book flickers.
    pub requote_cooldown_ms: u64,
}

impl Default for MarketMakerParams {
    fn default() -> Self {
        Self {
            symbol: "btcusdt".to_string(),
            qty_per_order: Decimal::new(1, 3), // 0.001
            spread_bps: 5, // 0.05% — tighter for testing (was 10)
            max_inventory: Decimal::new(10, 3), // 0.01
            imbalance_skew_factor: Decimal::new(5, 0), // 5 bps
            book_depth: 10,
            min_tick_move: Decimal::new(1, 2), // 0.01
            requote_cooldown_ms: 1000,        // don't re-quote more than once per second
        }
    }
}

/// Internal state behind a mutex (re-quote conditions + inventory from fill feedback).
struct State {
    last_mid: Option<Price>,
    last_regime: ImbalanceRegime,
    last_quoted_inventory: Decimal,
    /// Net position from fills: positive = long, negative = short (updated in on_fill).
    inventory: Decimal,
    /// Time of last re-quote (for cooldown).
    last_quoted_at: Option<std::time::Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            last_mid: None,
            last_regime: ImbalanceRegime::Neutral,
            last_quoted_inventory: Decimal::ZERO,
            inventory: Decimal::ZERO,
            last_quoted_at: None,
        }
    }
}

/// Market maker: post bid/ask around mid, skew by imbalance and inventory; re-quote on mid move, regime change, or inventory threshold.
pub struct MarketMakerStrategy {
    order_book: Arc<OrderBook>,
    params: MarketMakerParams,
    state: Mutex<State>,
}

impl MarketMakerStrategy {
    pub fn new(order_book: Arc<OrderBook>, params: MarketMakerParams) -> Self {
        Self {
            order_book,
            params,
            state: Mutex::new(State::default()),
        }
    }

    fn imbalance_regime(&self, bid_qty: Decimal, ask_qty: Decimal) -> ImbalanceRegime {
        let total = bid_qty + ask_qty;
        if total.is_zero() {
            return ImbalanceRegime::Neutral;
        }
        let ratio = (bid_qty - ask_qty) / total;
        // Use 5% threshold so small noise doesn't flip regime on every book update.
        if ratio > Decimal::new(5, 2) {
            ImbalanceRegime::BidHeavy
        } else if ratio < Decimal::new(-5, 2) {
            ImbalanceRegime::AskHeavy
        } else {
            ImbalanceRegime::Neutral
        }
    }

    fn should_requote(
        &self,
        mid: Price,
        regime: ImbalanceRegime,
        inventory: Decimal,
        state: &State,
    ) -> bool {
        if self.params.requote_cooldown_ms > 0 {
            if let Some(t) = state.last_quoted_at {
                if t.elapsed().as_millis() < self.params.requote_cooldown_ms as u128 {
                    return false;
                }
            }
        }
        let mid_moved = match state.last_mid {
            None => true,
            Some(lm) => (mid - lm).abs() >= self.params.min_tick_move,
        };
        let regime_changed = state.last_regime != regime;
        let inv_threshold = self.params.max_inventory / Decimal::from(4);
        let inventory_crossed =
            (inventory - state.last_quoted_inventory).abs() >= inv_threshold;

        mid_moved || regime_changed || inventory_crossed
    }

    fn emit_quotes(
        &self,
        buy_price: Price,
        sell_price: Price,
        signal_tx: &tokio::sync::mpsc::Sender<Signal>,
    ) {
        let symbol = self.params.symbol.clone();
        let qty = self.params.qty_per_order;
        let name = self.name().to_string();

        let buy_req = order_request(symbol.clone(), OrderSide::Buy, qty, Some(buy_price));
        let _ = signal_tx.try_send(Signal {
            request: buy_req,
            strategy_id: name.clone(),
            generated_at: std::time::Instant::now(),
        });

        let sell_req = order_request(symbol, OrderSide::Sell, qty, Some(sell_price));
        let _ = signal_tx.try_send(Signal {
            request: sell_req,
            strategy_id: name,
            generated_at: std::time::Instant::now(),
        });
    }
}

impl Strategy for MarketMakerStrategy {
    fn name(&self) -> &str {
        "market_maker"
    }

    fn on_orderbook_update(&self, _event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        let (bids, asks, _seq) = self.order_book.snapshot(self.params.book_depth);
        let mid = match self.order_book.mid_price() {
            Some(m) => m,
            None => return,
        };

        let bid_qty: Decimal = bids.iter().map(|l| l.qty).sum();
        let ask_qty: Decimal = asks.iter().map(|l| l.qty).sum();
        let regime = self.imbalance_regime(bid_qty, ask_qty);

        let (should_requote, inventory) = {
            let state = self.state.lock().unwrap();
            let inventory = state.inventory;
            let should = self.should_requote(mid, regime, inventory, &state);
            (should, inventory)
        };

        if !should_requote {
            return;
        }

        // Spread: half in each direction (spread_bps = 10 => 0.1% => 0.05% each side)
        let bps = Decimal::from(10_000);
        let half_spread_pct = (Decimal::from(self.params.spread_bps) / bps) / Decimal::from(2);
        let half_spread = mid * half_spread_pct;

        let mut buy_price = mid - half_spread;
        let mut sell_price = mid + half_spread;

        // Imbalance skew: bid-heavy → shift both up; ask-heavy → shift both down.
        let total = bid_qty + ask_qty;
        if !total.is_zero() {
            let imbalance_ratio = (bid_qty - ask_qty) / total;
            let shift_bps = imbalance_ratio * self.params.imbalance_skew_factor;
            let shift = mid * (shift_bps / bps);
            buy_price = buy_price + shift;
            sell_price = sell_price + shift;
        }

        // Inventory skew: too long → widen ask, tighten bid; too short → opposite.
        if !self.params.max_inventory.is_zero() {
            let inv_ratio = (inventory / self.params.max_inventory)
                .clamp(Decimal::from(-1), Decimal::from(1));
            let inv_bps = inv_ratio * Decimal::from(50); // up to ±50 bps
            let inv_shift = mid * (inv_bps / bps);
            // Long: bid up a bit (tighten), ask up more (widen). Short: bid down more, ask down a bit.
            buy_price = buy_price + inv_shift * Decimal::from(5) / Decimal::from(10);  // 0.5
            sell_price = sell_price + inv_shift * Decimal::from(15) / Decimal::from(10); // 1.5
        }

        self.emit_quotes(buy_price, sell_price, signal_tx);

        {
            let mut state = self.state.lock().unwrap();
            state.last_mid = Some(mid);
            state.last_regime = regime;
            state.last_quoted_inventory = inventory;
            state.last_quoted_at = Some(std::time::Instant::now());
        }
        debug!(
            mid = %mid,
            buy = %buy_price,
            sell = %sell_price,
            inventory = %inventory,
            "market_maker re-quoted"
        );
    }

    fn on_fill(&self, fill: &StrategyFill) {
        let mut state = self.state.lock().unwrap();
        let delta = match fill.side {
            OrderSide::Buy => fill.filled_qty,
            OrderSide::Sell => -fill.filled_qty,
        };
        state.inventory += delta;
    }
}
