//! Market maker strategy: post bid/ask around mid, skew by order-book imbalance and position inventory.
//!
//! ## Inventory model
//! Each fill creates or closes an [`OpenPosition`]. Positions are matched against the opposing
//! side on every new fill (worst unrealised-PnL first). Quote skew is driven by the count of
//! open positions per direction; when one direction is at `max_positions` only the reducing
//! side is quoted.
//!
//! ## Cancel reasons
//! Resting orders are cancelled on:
//!   DRIFT — mid moved >cancel_threshold_bps against the order
//!   LOSS  — total unrealised PnL exceeds cancel_loss_threshold and the order would worsen it

use hft_core::{OrderRequest, OrderSide, OrderType, Price};
use hft_feed_handler::FeedEvent;
use hft_order_book::OrderBook;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use super::{order_request, OrderAck, Signal, Strategy, StrategyFill};

// ── Imbalance regime ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImbalanceRegime {
    BidHeavy,
    AskHeavy,
    Neutral,
}

// ── Cancel reason ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelReason {
    /// Mid drifted against the order by more than cancel_threshold_bps.
    Drift,
    /// Total unrealised PnL is worse than cancel_loss_threshold and this order worsens it.
    Loss,
}

impl CancelReason {
    fn as_str(self) -> &'static str {
        match self {
            CancelReason::Drift => "DRIFT",
            CancelReason::Loss => "LOSS",
        }
    }
}

// ── Open position ─────────────────────────────────────────────────────────────

/// One open market-maker position — a filled order that has not yet been fully offset.
struct OpenPosition {
    /// Monotonic sequence number; lowest = oldest (used for FIFO ordering if needed).
    seq: u64,
    side: OrderSide,
    qty: Decimal,
    entry_price: Decimal,
    opened_at: std::time::Instant,
}

impl OpenPosition {
    /// Unrealised PnL at the given mark price.
    fn unrealized_pnl(&self, mark: Decimal) -> Decimal {
        let diff = match self.side {
            OrderSide::Buy  => mark - self.entry_price,
            OrderSide::Sell => self.entry_price - mark,
        };
        diff * self.qty
    }

    fn age_secs(&self) -> u64 {
        self.opened_at.elapsed().as_secs()
    }
}

// ── Pending (resting) order ───────────────────────────────────────────────────

struct PendingOrder {
    side: OrderSide,
    original_quote_price: Price,
    original_qty: Decimal,
    filled_qty: Decimal,
}

// ── Parameters ────────────────────────────────────────────────────────────────

/// Configurable parameters for the market maker strategy.
#[derive(Debug, Clone)]
pub struct MarketMakerParams {
    pub symbol: String,
    pub qty_per_order: Decimal,
    pub spread_bps: u64,
    pub imbalance_skew_factor: Decimal,
    pub book_depth: usize,
    pub min_tick_move: Decimal,
    pub requote_cooldown_ms: u64,

    /// Basis points of mid-price drift before a resting order is cancelled (DRIFT).
    /// e.g. 100 = 1 % move against the order triggers a cancel.
    pub cancel_threshold_bps: Decimal,

    /// Dollar loss threshold: cancel entry orders when total unrealised PnL
    /// is worse than this value (LOSS). e.g. 5.0 = $5 total unrealised loss.
    pub cancel_loss_threshold: Decimal,

    /// Seconds after strategy start during which no cancels are evaluated (warm-up).
    pub warmup_secs: u64,

    /// Minimum profit in bps required on exit orders relative to the average entry price.
    /// Prevents placing an exit that locks in a guaranteed loss. 0 = disabled.
    pub min_profit_bps: u64,

    // ── Inventory / position controls ────────────────────────────────────────

    /// Maximum open positions per direction (long OR short separately).
    /// When one direction reaches this cap only the reducing side is quoted.
    /// Default 5 (= 0.005 BTC at qty_per_order = 0.001).
    pub max_positions: usize,

    /// Additional quote skew applied per net position.
    /// Skew (bps) = skew_per_position_bps * |net_position_count|.
    /// Positive net (more longs) → lower bid and ask to encourage selling.
    /// Default 3 bps per position.
    pub skew_per_position_bps: u64,

    /// Fraction of max_positions (0–100) at which aggressive rebalancing skew activates.
    /// e.g. 60 = start extra skew once ≥ 60 % of max_positions is reached.
    pub rebalance_threshold_pct: u64,

    /// Additional quote improvement (bps) per step beyond the rebalance threshold.
    /// Applied to the exit side to encourage faster closure.
    pub aggressive_exit_bps: u64,

    /// Age in seconds after which a position is considered stale and the exit
    /// quote is progressively moved toward the inside to guarantee closure.
    /// 0 = disabled.
    pub max_position_age_secs: u64,
}

impl Default for MarketMakerParams {
    fn default() -> Self {
        Self {
            symbol: "btcusdt".to_string(),
            qty_per_order: Decimal::new(1, 3),       // 0.001 BTC
            spread_bps: 5,
            imbalance_skew_factor: Decimal::new(5, 0),
            book_depth: 10,
            min_tick_move: Decimal::new(1, 2),        // $0.01
            requote_cooldown_ms: 1000,
            cancel_threshold_bps: Decimal::new(100, 0), // 1 %
            cancel_loss_threshold: Decimal::new(5, 0),  // $5 total unrealised
            warmup_secs: 5,
            min_profit_bps: 3,
            max_positions: 5,
            skew_per_position_bps: 3,
            rebalance_threshold_pct: 60,
            aggressive_exit_bps: 5,
            max_position_age_secs: 30,
        }
    }
}

// ── Strategy state ────────────────────────────────────────────────────────────

struct State {
    last_mid: Option<Price>,
    last_regime: ImbalanceRegime,
    /// Position counts at the time of the last requote (used to detect change).
    last_quoted_long: usize,
    last_quoted_short: usize,
    last_quoted_at: Option<std::time::Instant>,

    /// All open positions (long and short).
    positions: Vec<OpenPosition>,
    next_seq: u64,
    /// Cumulative realised PnL from closed positions.
    realized_pnl: Decimal,

    /// Resting orders: order_id → PendingOrder.
    pending_orders: HashMap<String, PendingOrder>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            last_mid: None,
            last_regime: ImbalanceRegime::Neutral,
            last_quoted_long: 0,
            last_quoted_short: 0,
            last_quoted_at: None,
            positions: Vec::new(),
            next_seq: 0,
            realized_pnl: Decimal::ZERO,
            pending_orders: HashMap::new(),
        }
    }
}

impl State {
    fn long_count(&self) -> usize {
        self.positions.iter().filter(|p| matches!(p.side, OrderSide::Buy)).count()
    }

    fn short_count(&self) -> usize {
        self.positions.iter().filter(|p| matches!(p.side, OrderSide::Sell)).count()
    }

    fn net_qty(&self) -> Decimal {
        self.positions.iter().map(|p| match p.side {
            OrderSide::Buy  =>  p.qty,
            OrderSide::Sell => -p.qty,
        }).sum()
    }

    fn unrealized_pnl(&self, mid: Decimal) -> Decimal {
        self.positions.iter().map(|p| p.unrealized_pnl(mid)).sum()
    }

    fn total_pnl(&self, mid: Decimal) -> Decimal {
        self.realized_pnl + self.unrealized_pnl(mid)
    }

    /// Weighted average entry price for all positions on the given side.
    fn avg_entry_for_side(&self, side: OrderSide) -> Decimal {
        let total_qty: Decimal = self.positions.iter()
            .filter(|p| p.side == side)
            .map(|p| p.qty)
            .sum();
        if total_qty.is_zero() { return Decimal::ZERO; }
        let total_cost: Decimal = self.positions.iter()
            .filter(|p| p.side == side)
            .map(|p| p.entry_price * p.qty)
            .sum();
        total_cost / total_qty
    }

    /// Age in seconds of the oldest position on the given side (0 if none).
    fn oldest_age_secs(&self, side: OrderSide) -> u64 {
        self.positions.iter()
            .filter(|p| p.side == side)
            .map(|p| p.age_secs())
            .max()
            .unwrap_or(0)
    }
}

// ── Strategy struct ───────────────────────────────────────────────────────────

pub struct MarketMakerStrategy {
    order_book: Arc<OrderBook>,
    params: MarketMakerParams,
    state: Mutex<State>,
    started_at: std::time::Instant,
}

impl MarketMakerStrategy {
    pub fn new(order_book: Arc<OrderBook>, params: MarketMakerParams) -> Self {
        Self {
            order_book,
            params,
            state: Mutex::new(State::default()),
            started_at: std::time::Instant::now(),
        }
    }

    fn imbalance_regime(&self, bid_qty: Decimal, ask_qty: Decimal) -> ImbalanceRegime {
        let total = bid_qty + ask_qty;
        if total.is_zero() { return ImbalanceRegime::Neutral; }
        let ratio = (bid_qty - ask_qty) / total;
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
        long_count: usize,
        short_count: usize,
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
        // Trigger requote whenever the position count on either side changes.
        let position_changed =
            long_count  != state.last_quoted_long ||
            short_count != state.last_quoted_short;

        mid_moved || regime_changed || position_changed
    }

    /// Evaluate whether a resting order should be cancelled.
    fn cancel_reason(
        &self,
        order: &PendingOrder,
        mid: Price,
        net_qty: Decimal,
        unrealized_pnl: Decimal,
    ) -> Option<CancelReason> {
        if self.started_at.elapsed().as_secs() < self.params.warmup_secs {
            return None;
        }

        // LOSS: total unrealised PnL is worse than the threshold AND this order would
        // deepen the losing side.
        if !net_qty.is_zero() && unrealized_pnl < -self.params.cancel_loss_threshold {
            let worsens = match order.side {
                OrderSide::Buy  => net_qty > Decimal::ZERO, // long and losing: don't buy more
                OrderSide::Sell => net_qty < Decimal::ZERO, // short and losing: don't sell more
            };
            if worsens {
                return Some(CancelReason::Loss);
            }
        }

        // DRIFT: mid moved against the order by more than cancel_threshold_bps.
        let price_gap = match order.side {
            OrderSide::Buy  => order.original_quote_price - mid,
            OrderSide::Sell => mid - order.original_quote_price,
        };
        let threshold = mid * (self.params.cancel_threshold_bps / Decimal::from(10_000));
        if price_gap > threshold {
            return Some(CancelReason::Drift);
        }

        None
    }

    fn emit_one(&self, side: OrderSide, price: Price, signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        let req = order_request(
            self.params.symbol.clone(), side, self.params.qty_per_order, Some(price),
        );
        if signal_tx.try_send(Signal {
            request: req,
            strategy_id: self.name().to_string(),
            generated_at: std::time::Instant::now(),
        }).is_err() {
            warn!(strategy = "market_maker", side = ?side, "signal channel full, quote dropped");
        }
    }

    fn emit_cancel(
        &self,
        order_id: &str,
        side: OrderSide,
        original_price: Price,
        reason: CancelReason,
        mid: Price,
        signal_tx: &tokio::sync::mpsc::Sender<Signal>,
    ) {
        let req = OrderRequest {
            symbol: self.params.symbol.clone(),
            side,
            order_type: OrderType::Cancel,
            qty: Decimal::ZERO,
            price: Some(original_price),
            time_in_force: None,
            client_order_id: Some(order_id.to_string()),
            cancel_reason: Some(reason.as_str().to_string()),
            cancel_eval_mid: Some(mid),
        };
        if signal_tx.try_send(Signal {
            request: req,
            strategy_id: self.name().to_string(),
            generated_at: std::time::Instant::now(),
        }).is_err() {
            warn!(strategy = "market_maker", order_id, "signal channel full, cancel dropped");
        }
    }
}

// ── Strategy trait impl ───────────────────────────────────────────────────────

impl Strategy for MarketMakerStrategy {
    fn name(&self) -> &str { "market_maker" }

    fn on_order_ack(&self, ack: &OrderAck) {
        if let Some(price) = ack.price {
            let mut state = self.state.lock().unwrap();
            state.pending_orders.insert(ack.order_id.clone(), PendingOrder {
                side: ack.side,
                original_quote_price: price,
                original_qty: self.params.qty_per_order,
                filled_qty: Decimal::ZERO,
            });
            debug!(order_id = %ack.order_id, side = ?ack.side, price = %price,
                   "market_maker: tracking pending order");
        }
    }

    fn on_orderbook_update(&self, _event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>) {
        let (bids, asks, _seq) = self.order_book.snapshot(self.params.book_depth);

        // Single-snapshot best_bid/best_ask + mid — avoids the race from two separate
        // lock acquisitions that can produce a mid inconsistent with the imbalance data.
        let best_bid = match bids.first() { Some(l) => l.price, None => return };
        let best_ask = match asks.first() { Some(l) => l.price, None => return };
        if best_bid >= best_ask { return; }
        let mid = (best_bid + best_ask) / Decimal::from(2);

        let bid_qty: Decimal = bids.iter().map(|l| l.qty).sum();
        let ask_qty: Decimal = asks.iter().map(|l| l.qty).sum();
        let regime = self.imbalance_regime(bid_qty, ask_qty);

        // ── Snapshot position state ────────────────────────────────────────────
        // Read everything we need from state in one lock scope so all values are
        // consistent with each other.
        let (long_count, short_count, net_qty, unrealized_pnl, realized_pnl,
             avg_long_entry, avg_short_entry,
             oldest_long_secs, oldest_short_secs) = {
            let state = self.state.lock().unwrap();
            (
                state.long_count(),
                state.short_count(),
                state.net_qty(),
                state.unrealized_pnl(mid),
                state.realized_pnl,
                state.avg_entry_for_side(OrderSide::Buy),
                state.avg_entry_for_side(OrderSide::Sell),
                state.oldest_age_secs(OrderSide::Buy),
                state.oldest_age_secs(OrderSide::Sell),
            )
        };
        let total_pnl = realized_pnl + unrealized_pnl;

        // Warn if inventory exceeds safe bounds.
        let max_pos = self.params.max_positions;
        if long_count > max_pos || short_count > max_pos {
            warn!(
                long_count, short_count, max_pos,
                unrealized_pnl = %unrealized_pnl,
                total_pnl = %total_pnl,
                "market_maker: position count exceeds max_positions — inventory risk"
            );
        }

        // ── Stale order detection ─────────────────────────────────────────────
        let stale_orders: Vec<(String, OrderSide, Price, CancelReason)> = {
            let state = self.state.lock().unwrap();
            state.pending_orders.iter()
                .filter_map(|(id, order)| {
                    self.cancel_reason(order, mid, net_qty, unrealized_pnl)
                        .map(|r| (id.clone(), order.side, order.original_quote_price, r))
                })
                .collect()
        };

        if !stale_orders.is_empty() {
            let mut state = self.state.lock().unwrap();
            for (id, side, price, reason) in &stale_orders {
                state.pending_orders.remove(id);
                warn!(
                    order_id = %id, side = ?side, original_price = %price,
                    current_mid = %mid, reason = reason.as_str(),
                    "market_maker: cancelling stale order"
                );
            }
        }
        for (id, side, price, reason) in &stale_orders {
            self.emit_cancel(id, *side, *price, *reason, mid, signal_tx);
        }

        // ── Re-quote decision ─────────────────────────────────────────────────
        let should_requote = {
            let had_cancel = !stale_orders.is_empty();
            let state = self.state.lock().unwrap();
            had_cancel || self.should_requote(mid, regime, long_count, short_count, &state)
        };
        if !should_requote { return; }

        // ── Base quotes: mid ± half_spread ────────────────────────────────────
        let bps = Decimal::from(10_000);
        let half_spread = mid * ((Decimal::from(self.params.spread_bps) / bps) / Decimal::from(2));
        let mut buy_price  = mid - half_spread;
        let mut sell_price = mid + half_spread;

        // ── Order-book imbalance skew (shifts both prices equally) ────────────
        let total_book_qty = bid_qty + ask_qty;
        if !total_book_qty.is_zero() {
            let imbalance_ratio = (bid_qty - ask_qty) / total_book_qty;
            let shift = mid * (imbalance_ratio * self.params.imbalance_skew_factor / bps);
            buy_price  += shift;
            sell_price += shift;
        }

        // ── Position-count inventory skew (asymmetric) ────────────────────────
        // Skew magnitude = skew_per_position_bps × |net_position_count|.
        // Applied asymmetrically: 1.5× on the entry side, 0.5× on the exit side.
        // This widens the entry (discourages more of the same side) while gently
        // moving the exit closer to market (encourages faster closure).
        if self.params.max_positions > 0 && self.params.skew_per_position_bps > 0 {
            let net_pos = long_count as i64 - short_count as i64;
            let clamped  = net_pos.clamp(-(max_pos as i64), max_pos as i64);
            let skew_bps = Decimal::from(self.params.skew_per_position_bps as i64 * clamped);
            let inv_shift = mid * (skew_bps / bps);
            buy_price  -= inv_shift * Decimal::new(15, 1); // 1.5× on bid
            sell_price -= inv_shift * Decimal::new(5, 1);  // 0.5× on ask
        }

        // ── Rebalancing skew: extra improvement when approaching max_positions ─
        // Once position count reaches rebalance_threshold_pct % of max_positions,
        // each additional position adds aggressive_exit_bps of improvement to the
        // exit side, accelerating closure without hard cancels or market orders.
        let rebalance_threshold =
            (max_pos as u64 * self.params.rebalance_threshold_pct / 100) as usize;

        if long_count >= rebalance_threshold && long_count > 0 {
            let steps = (long_count.saturating_sub(rebalance_threshold) + 1) as u64;
            let extra = mid * (Decimal::from(self.params.aggressive_exit_bps * steps) / bps);
            sell_price -= extra; // lower ask → fills sooner → reduces long inventory
            debug!(long_count, steps, extra=%extra, "market_maker: rebalance skew applied to sell");
        }
        if short_count >= rebalance_threshold && short_count > 0 {
            let steps = (short_count.saturating_sub(rebalance_threshold) + 1) as u64;
            let extra = mid * (Decimal::from(self.params.aggressive_exit_bps * steps) / bps);
            buy_price += extra; // raise bid → fills sooner → reduces short inventory
            debug!(short_count, steps, extra=%extra, "market_maker: rebalance skew applied to buy");
        }

        // ── Aged-position emergency exit ──────────────────────────────────────
        // When the oldest position on a side exceeds max_position_age_secs, the
        // exit quote is progressively moved toward the inside to guarantee closure.
        // Each additional age-period adds another aggressive_exit_bps of improvement.
        let age_limit = self.params.max_position_age_secs;
        if age_limit > 0 {
            if oldest_long_secs >= age_limit {
                let periods = (oldest_long_secs / age_limit).min(20); // cap multiplier
                let extra = mid * (Decimal::from(self.params.aggressive_exit_bps * periods) / bps);
                sell_price -= extra;
                warn!(
                    oldest_long_secs, periods, sell_price = %sell_price,
                    "market_maker: aged long position — emergency sell skew"
                );
            }
            if oldest_short_secs >= age_limit {
                let periods = (oldest_short_secs / age_limit).min(20);
                let extra = mid * (Decimal::from(self.params.aggressive_exit_bps * periods) / bps);
                buy_price += extra;
                warn!(
                    oldest_short_secs, periods, buy_price = %buy_price,
                    "market_maker: aged short position — emergency buy skew"
                );
            }
        }

        // ── Minimum exit price enforcement ────────────────────────────────────
        // Prevent placing an exit order that locks in a guaranteed loss relative
        // to the average entry of the position being closed.
        if self.params.min_profit_bps > 0 {
            let profit_factor = Decimal::from(self.params.min_profit_bps) / bps;
            if avg_long_entry > Decimal::ZERO {
                let min_sell = avg_long_entry + avg_long_entry * profit_factor;
                if sell_price < min_sell {
                    debug!(sell=%sell_price, min_sell=%min_sell, "market_maker: sell raised to min-profit floor");
                    sell_price = min_sell;
                }
            }
            if avg_short_entry > Decimal::ZERO {
                let max_buy = avg_short_entry - avg_short_entry * profit_factor;
                if buy_price > max_buy {
                    debug!(buy=%buy_price, max_buy=%max_buy, "market_maker: buy lowered to min-profit ceiling");
                    buy_price = max_buy;
                }
            }
        }

        // ── Passive order guard ───────────────────────────────────────────────
        // Hard rule: buy must be strictly below best_ask (passive maker on bid side),
        //            sell must be strictly above best_bid (passive maker on ask side).
        // The old `mid ± $0.01` clamp was incorrect: for a tight spread it placed
        // orders inside the spread but above/below the BBO, risking taker fills.
        let tick = Decimal::new(1, 2); // $0.01

        if buy_price >= best_ask {
            warn!(buy=%buy_price, best_ask=%best_ask, "market_maker: buy would cross — clamping");
            buy_price = best_ask - tick;
        }
        if sell_price <= best_bid {
            warn!(sell=%sell_price, best_bid=%best_bid, "market_maker: sell would cross — clamping");
            sell_price = best_bid + tick;
        }
        // Secondary: keep each quote on its passive side of mid.
        if buy_price >= mid  { buy_price  = mid - tick; }
        if sell_price <= mid { sell_price = mid + tick; }

        // Final crossed-quote abort.
        if buy_price >= sell_price {
            warn!(buy=%buy_price, sell=%sell_price, "market_maker: quotes still crossed — aborting");
            return;
        }
        // Paranoia: double-check neither quote slipped past the spread guards.
        if buy_price >= best_ask || sell_price <= best_bid {
            warn!(buy=%buy_price, sell=%sell_price, best_bid=%best_bid, best_ask=%best_ask,
                  "market_maker: quote crosses spread after all guards — aborting");
            return;
        }

        // ── Emit quotes (one-sided when at position cap) ──────────────────────
        let at_max_long  = long_count  >= self.params.max_positions;
        let at_max_short = short_count >= self.params.max_positions;

        if at_max_long && at_max_short {
            // Fully maxed on both sides — nothing safe to quote. Wait for a fill.
            debug!(long_count, short_count, "market_maker: max positions both sides — skipping quote");
        } else if at_max_long {
            self.emit_one(OrderSide::Sell, sell_price, signal_tx);
            debug!(mid=%mid, sell=%sell_price, long_count, "market_maker: max long — ask only");
        } else if at_max_short {
            self.emit_one(OrderSide::Buy, buy_price, signal_tx);
            debug!(mid=%mid, buy=%buy_price, short_count, "market_maker: max short — bid only");
        } else {
            self.emit_one(OrderSide::Buy,  buy_price,  signal_tx);
            self.emit_one(OrderSide::Sell, sell_price, signal_tx);
        }

        // ── Update state ──────────────────────────────────────────────────────
        {
            let mut state = self.state.lock().unwrap();
            state.last_mid              = Some(mid);
            state.last_regime           = regime;
            state.last_quoted_long      = long_count;
            state.last_quoted_short     = short_count;
            state.last_quoted_at        = Some(std::time::Instant::now());
        }

        // ── Diagnostic log ────────────────────────────────────────────────────
        info!(
            mid          = %mid,
            best_bid     = %best_bid,
            best_ask     = %best_ask,
            buy          = %buy_price,
            sell         = %sell_price,
            long_count,
            short_count,
            net_qty      = %net_qty,
            unrealized   = %unrealized_pnl,
            realized     = %realized_pnl,
            total_pnl    = %total_pnl,
            at_max_long,
            at_max_short,
            "market_maker: re-quoted"
        );
    }

    fn on_fill(&self, fill: &StrategyFill) {
        let mut state = self.state.lock().unwrap();

        // ── Update pending order tracking ─────────────────────────────────────
        if let Some(oid) = &fill.order_id {
            if let Some(po) = state.pending_orders.get_mut(oid) {
                po.filled_qty += fill.filled_qty;
                if po.filled_qty >= po.original_qty {
                    state.pending_orders.remove(oid);
                }
            }
        }

        let mark = fill.fill_price; // mark price for PnL sorting
        let mut remaining_qty = fill.filled_qty;

        // ── Close opposing positions (worst unrealised PnL first) ────────────
        // A BUY fill closes SHORT positions; a SELL fill closes LONG positions.
        let closes_side = match fill.side {
            OrderSide::Buy  => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        };

        // Collect indices of positions on the side being closed, sorted by
        // ascending unrealised PnL (most losing / most underwater first).
        let mut close_indices: Vec<usize> = state.positions.iter().enumerate()
            .filter(|(_, p)| p.side == closes_side)
            .map(|(i, _)| i)
            .collect();
        close_indices.sort_unstable_by(|&a, &b| {
            let pnl_a = state.positions[a].unrealized_pnl(mark);
            let pnl_b = state.positions[b].unrealized_pnl(mark);
            pnl_a.cmp(&pnl_b)
        });

        let mut to_remove: Vec<usize> = Vec::new();
        for idx in close_indices {
            if remaining_qty.is_zero() { break; }
            let pos_qty   = state.positions[idx].qty;
            let pos_entry = state.positions[idx].entry_price;
            let close_qty = remaining_qty.min(pos_qty);

            // Realise PnL for this (partial) close.
            let pnl = match fill.side {
                OrderSide::Buy  => (pos_entry - mark) * close_qty, // closing short
                OrderSide::Sell => (mark - pos_entry) * close_qty, // closing long
            };
            state.realized_pnl += pnl;
            remaining_qty -= close_qty;

            if close_qty >= state.positions[idx].qty {
                to_remove.push(idx);
            } else {
                state.positions[idx].qty -= close_qty;
            }
        }
        // Remove fully-closed positions (in reverse index order to preserve indexing).
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in to_remove {
            state.positions.remove(idx);
        }

        // ── Any remaining qty opens a new position ────────────────────────────
        if remaining_qty > Decimal::ZERO {
            let seq = state.next_seq;
            state.next_seq += 1;
            state.positions.push(OpenPosition {
                seq,
                side: fill.side,
                qty: remaining_qty,
                entry_price: mark,
                opened_at: std::time::Instant::now(),
            });
        }

        // ── Diagnostic log ────────────────────────────────────────────────────
        let long_c    = state.long_count();
        let short_c   = state.short_count();
        let net       = state.net_qty();
        let realized  = state.realized_pnl;
        // Approximate unrealised using fill price as mark (best available without book lock).
        let unrealized = state.unrealized_pnl(mark);
        let total      = realized + unrealized;

        info!(
            fill_side       = ?fill.side,
            fill_price      = %mark,
            fill_qty        = %fill.filled_qty,
            long_positions  = long_c,
            short_positions = short_c,
            net_qty         = %net,
            realized_pnl    = %realized,
            unrealized_pnl  = %unrealized,
            total_pnl       = %total,
            "market_maker: fill processed"
        );
    }
}
