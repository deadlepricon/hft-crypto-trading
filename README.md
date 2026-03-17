# HFT Crypto Trading System

High-frequency trading style cryptocurrency system with a Rust backend and terminal UI. Designed for low latency, high throughput, and modular extension.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full design, data flow, and folder structure.

**Pipeline:** Exchange WebSockets → Feed Handler → Order Book → Strategy Engine → Risk Manager → Execution Engine → Exchange.

**Crates:**

| Crate | Responsibility |
|-------|----------------|
| `hft-core` | Shared types, events, errors |
| `hft-order-book` | In-memory order book |
| `hft-exchange` | Exchange connectors (Simulator, Binance stub) |
| `hft-feed-handler` | Ingest and normalize market data |
| `hft-strategy` | Strategy engine and strategy modules |
| `hft-risk` | Position limits, exposure, order validation |
| `hft-execution` | Order lifecycle and fills |
| `hft-metrics` | Latency and throughput metrics |
| `hft-logging` | Persist trades and events |
| `hft-backtesting` | Replay historical data, evaluate strategies |
| `hft-ui` | Terminal UI (ratatui) |
| `hft` | Main binary |

## Build and Run

```bash
cargo build --release
cargo run --release -p hft
```

Press `q` to quit the TUI.

### Exchange: Simulator vs Live (same code path)

The same binary and pipeline run for both the **local simulator** and **live** exchanges. Choose the backend with the `EXCHANGE` env var:

| `EXCHANGE`   | Backend   | Use case |
|-------------|-----------|----------|
| `simulator` (default) | Local simulator | Development, testing |
| `binance`   | Binance (stub)  | Live (when implemented) |

**Simulator (default)**  
1. Start your exchange simulator (e.g. WebSocket at `ws://localhost:8765/ws/feed`, REST at `http://localhost:8765`).
2. Run: `cargo run --release -p hft` (or set `EXCHANGE=simulator`).
3. Symbol is `BTC/USDT` by default (override with `SYMBOL`).

Simulator data: **book** = synthetic (for local matching); **trades** = mostly live from Binance + your fills (Binance timestamps are ms epoch string; simulator fills may have `null`); **ticker** = from Binance last trade (we parse book + trades; ticker can be added if needed).

Optional env overrides:
- `STRATEGIES` — Comma-separated strategy names (default: `market_maker`). Supported: `market_maker`, `imbalance`. Use e.g. `market_maker,imbalance` to run both in parallel.
- `SIMULATOR_BASE_URL` — REST base (default: `http://localhost:8765`)
- `SIMULATOR_WS_URL` — WebSocket URL (default: `ws://localhost:8765/ws/feed`)

### Market maker (default strategy)

The primary strategy is **market maker**: it posts a **buy** limit below mid and a **sell** limit above mid, then adjusts both with:

- **Spread** — `spread_bps` (e.g. 10 = 0.1%) so bid is below mid and ask above mid.
- **Imbalance skew** — If the book is bid-heavy, both quotes shift up (more aggressive to sell); if ask-heavy, both shift down (more aggressive to buy). Uses top `book_depth` levels and `imbalance_skew_factor`.
- **Inventory skew** — Tracks position from fills. Long: widen ask and tighten bid to encourage selling; short: opposite. Capped by `max_inventory`.

**Re-quote** only when mid moves by `min_tick_move`, imbalance regime changes, or inventory crosses a threshold; plus a `requote_cooldown_ms` to avoid spamming. **Quantity** per order is fixed: `qty_per_order` (e.g. 0.001). All parameters live in `MarketMakerParams` in `hft-strategy/src/strategies/market_maker.rs`; the registry uses defaults and you can extend it to accept config later.

**Live (Binance)**  
- Run with `EXCHANGE=binance` and set `SYMBOL` (e.g. `btcusdt`) when the Binance connector is fully implemented.

## UI Panels

- **Order book** – Bids and asks (left)
- **Recent trades** – Last trades (left)
- **PnL & latency** – Cumulative PnL, win rate, feed latency, message count
- **Positions** – Current positions with entry and unrealized PnL
- **System logs** – Recent log lines

## Extending

- **New exchange:** Implement `ExchangeConnector` in `hft-exchange` and parse exchange-specific JSON into `ExchangeMessage`.
- **New strategy:** Implement `Strategy` in `hft-strategy/strategies`, add a constructor in `hft-strategy/registry.rs`, then run it by name via the `STRATEGIES` env var.
- **Live pipeline:** In `hft/src/main.rs`, spawn `feed_handler.run()`, strategy engine, risk manager, and execution engine as Tokio tasks and connect their channels; run the TUI in a thread and pass shared `App` state (e.g. via `Arc<RwLock<App>>` or channels).

## Tech Stack

- **Runtime:** Tokio
- **TUI:** ratatui + crossterm
- **Decimals:** rust_decimal
- **Serialization:** serde, serde_json
