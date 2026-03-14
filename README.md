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

Optional env overrides for the simulator:
- `SIMULATOR_BASE_URL` — REST base (default: `http://localhost:8765`)
- `SIMULATOR_WS_URL` — WebSocket URL (default: `ws://localhost:8765/ws/feed`)

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
- **New strategy:** Implement `Strategy` in `hft-strategy/strategies` and register with `StrategyEngine`.
- **Live pipeline:** In `hft/src/main.rs`, spawn `feed_handler.run()`, strategy engine, risk manager, and execution engine as Tokio tasks and connect their channels; run the TUI in a thread and pass shared `App` state (e.g. via `Arc<RwLock<App>>` or channels).

## Tech Stack

- **Runtime:** Tokio
- **TUI:** ratatui + crossterm
- **Decimals:** rust_decimal
- **Serialization:** serde, serde_json
