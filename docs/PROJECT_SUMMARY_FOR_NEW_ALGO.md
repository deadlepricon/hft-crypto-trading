# HFT Crypto Trading System — Project Summary for New Trading Algorithm

Use this summary to brief another model so it can design and implement a new trading strategy that plugs into this codebase.

---

## 1. High-level architecture

- **Language:** Rust. Workspace with multiple crates under `crates/`.
- **Pipeline:** Feed → Strategy → Risk → Execution. Single code path; paper vs live is only at execution (paper = simulated fill at mid, live = real exchange).
- **Threading:** Feed handler runs on its own thread; strategy + risk + execution run together on a second thread; UI runs on the main thread. Trading logic is isolated from UI.

---

## 2. Data flow

1. **Exchange connector** (e.g. Binance.US WebSocket) produces raw messages.
2. **Feed handler** (`hft-feed-handler`): normalizes messages into `FeedEvent`, updates a shared `OrderBook`, and **broadcasts** `EventEnvelope<FeedEvent>` so multiple subscribers can receive the same stream.
3. **Strategy engine** subscribes to the feed and, for each event, calls each registered strategy’s `on_feed_event`. Strategies may send **Signals** (order requests) on a channel.
4. **Risk manager** consumes signals, checks limits, and forwards approved **OrderRequest**s to execution.
5. **Execution engine** either submits to the exchange (live) or simulates an immediate fill at order book mid and sends **PaperFill** events (paper). Paper fills go to the UI and to a **PositionTracker** for PnL.

---

## 3. Market data: FeedEvent and OrderBook

**FeedEvent** (from `hft-feed-handler`):

- `OrderBookSnapshot(OrderBookSnapshot)` — full book replace
- `OrderBookDelta(OrderBookDelta)` — incremental book update
- `Trade(TradeEvent)` — public trade (symbol, side, price, qty)
- `Ticker(TickerUpdate)` — last price, 24h volume, best bid/ask

Strategies receive **references** to these events. The **OrderBook** is a shared `Arc<OrderBook>` that the feed handler updates; strategies read from it (e.g. `order_book.snapshot(depth)`, `order_book.best_bid()`, `order_book.mid_price()`). Types like `Level` have `price: Price` and `qty: Qty` (both `rust_decimal::Decimal`). Symbol is typically `"btcusdt"`.

---

## 4. Strategy interface (how to add a new algo)

**Crate:** `hft-strategy`.

**Trait:** `Strategy: Send + Sync` with:

- `fn name(&self) -> &str`
- `fn on_feed_event(&self, event: &FeedEvent, signal_tx: &tokio::sync::mpsc::Sender<Signal>)`  
  Default implementation dispatches to:
  - `on_orderbook_update(event, signal_tx)` for snapshot/delta
  - `on_trade(event, signal_tx)` for trades
  - `on_ticker_update(event, signal_tx)` for ticker  
  Override the specific callbacks you need; default no-ops for the rest.

**Signal:** A trading signal is:

```rust
pub struct Signal {
    pub request: OrderRequest,
    pub strategy_id: String,
    pub generated_at: Instant,
}
```

**OrderRequest** (from `hft-core`):

- `symbol: String`
- `side: OrderSide` (Buy / Sell)
- `order_type: OrderType` (Limit / Market)
- `qty: Qty` (Decimal)
- `price: Option<Price>` (Decimal; None for market)
- `time_in_force: Option<TimeInForce>`

**Helper:** `order_request(symbol, side, qty, price)` in `hft-strategy` builds an `OrderRequest` (price `Some` => Limit, `None` => Market).

Strategies are registered as `Arc<dyn Strategy>`. The engine calls `on_feed_event` for **every** feed event; strategies must be efficient and non-blocking. Use `signal_tx.try_send(signal)` so you don’t block if the channel is full.

---

## 5. Execution and paper trading

- **Paper mode (default):** Execution engine gets approved `OrderRequest`s, fills at **order book mid** (or best bid/ask if no mid), updates a **PositionTracker** (realized + unrealized PnL), and sends **PaperFill** (request, fill_price, pnl_delta, is_buy, unrealized_pnl) to the UI. No orders sent to the exchange.
- **Live mode:** Same pipeline, but execution uses an exchange connector to submit orders; no PaperFill channel.
- **PositionTracker** trait: `apply_fill(req, fill_price) -> (realized_pnl_delta, is_buy, unrealized_pnl)`. Implementations track position and average entry per symbol (long/short, fractional qty).

Strategies do **not** receive fill callbacks or position state; they only see feed events and send signals. Position and PnL are handled in the execution/UI layer.

---

## 6. Risk

Risk manager sits between strategy and execution. It receives `Signal`s and can reject or modify; approved orders become `OrderRequest`s. Risk limits and exact behavior are in `hft-risk`. New strategies don’t need to know internals; they just send signals.

---

## 7. Current strategy: Imbalance (reference)

The only active strategy so far is **ImbalanceStrategy** in `crates/hft-strategy/src/strategies/imbalance.rs`:

- Sums top N bid levels and top N ask levels from the order book.
- **Regime:** BidHeavy (imbalance ≥ threshold_buy), AskHeavy (imbalance ≤ -threshold_sell), else Neutral.
- **Signals:** One **Buy** when **entering** BidHeavy, one **Sell** when **entering** AskHeavy (transition-only; no repeat signals while staying in the same regime).
- Optional **asymmetric thresholds** (e.g. higher sell threshold, lower buy threshold) and **confirm_ticks** (require regime to persist for N book updates before signalling).
- Uses a shared `OrderBook` and `ImbalanceParams` (book_depth, thresholds, order_size, use_limit, confirm_ticks).

Use it as a template for a new strategy: same trait, same `order_request` helper, same pattern of reading book and sending `Signal`s.

---

## 8. Wiring a new strategy in main

In `crates/hft/src/main.rs`:

- Create `(signal_tx, signal_rx)` and `(order_tx, order_rx)`.
- Build your strategy (e.g. `Arc::new(MyStrategy::new(...))`).
- `engine.add_strategy(my_strategy)`.
- Strategy engine is run with `feed_rx_engine` (a broadcast receiver of feed events); risk consumes `signal_rx`; execution consumes `order_rx`.

You can run **multiple** strategies; each gets every feed event and can send signals. Risk/execution see a single stream of signals/orders.

---

## 9. Crates overview

- **hft** — main binary; wires feed, strategy engine, risk, execution, UI.
- **hft-core** — types: OrderRequest, OrderSide, Price, Qty, Level, etc.; events (EventEnvelope, OrderBookSnapshot, OrderBookDelta, TradeEvent).
- **hft-exchange** — ExchangeConnector trait; Binance.US WebSocket implementation; ExchangeMessage (OrderBookSnapshot, OrderBookDelta, Trade, etc.).
- **hft-feed-handler** — FeedHandler, FeedEvent, normalizes exchange messages and broadcasts.
- **hft-order-book** — OrderBook (replace, apply delta), snapshot(depth), best_bid(), best_ask(), mid_price().
- **hft-strategy** — Strategy trait, Signal, StrategyEngine, order_request(); ImbalanceStrategy lives here.
- **hft-risk** — RiskManager, RiskLimits; consumes signals, produces OrderRequests.
- **hft-execution** — ExecutionEngine, PaperFill, PositionTracker; paper vs live.
- **hft-ui** — TUI (ratatui): order book, recent trades, positions, PnL & performance, logs, price feed.
- **hft-metrics** — atomic counters (fills, trades received, latency, etc.).
- **hft-backtesting, hft-optimizer, hft-experiment-runner, hft-data-recorder, hft-performance-metrics** — present for backtest/optimization/experiments; not required to add a new live/paper strategy.

---

## 10. What to ask the other model to produce

Ask it to:

1. **Design a new trading algorithm** that fits the above: it consumes `FeedEvent` (and optionally the shared `OrderBook`), and emits `Signal` (i.e. `OrderRequest`) via the given `signal_tx`. No fill or position feedback; paper PnL is handled by the existing execution/position tracker.
2. **Implement it** as a new strategy in `hft-strategy`: a new file under `strategies/`, implementing `Strategy`, with a params struct and a constructor that takes `Arc<OrderBook>`, symbol, and params (and any other needed deps).
3. **Wire it in `main.rs`**: instantiate the new strategy and `engine.add_strategy(arc_new_strategy)` (and remove or keep the existing ImbalanceStrategy as desired).
4. **Use existing types** only: `FeedEvent`, `OrderRequest`, `OrderSide`, `Signal`, `order_request`, `OrderBook` snapshot/best_bid/best_ask/mid_price, `Decimal` for prices and quantities.

If the new algo needs **position or fill feedback**, that would require an extension (e.g. a channel or shared state from execution back into the strategy crate); the current design does not provide it.

---

## 11. Running the app

- Run from a real terminal (TTY). Env: `PAPER_TRADING` (default true), `EXCHANGE` (e.g. binance), `SYMBOL` (e.g. btcusdt).
- Paper mode: no exchange orders; fills simulated at mid; PnL and “Our fills” update in the UI.
- Exit: key `q`.
