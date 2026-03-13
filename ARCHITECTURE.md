# HFT Crypto Trading System — Architecture

## Overview

High-frequency trading style cryptocurrency system with a Rust backend and terminal UI. The design prioritizes **low latency**, **high throughput**, and **modularity** so components can be evolved or replaced independently.

## Data Flow (Pipeline)

```
Exchange WebSockets
        │
        ▼
┌───────────────────┐     market data      ┌───────────────────┐
│  Feed Handler     │─────────────────────▶│  Order Book       │
│  (normalize,      │                      │  (in-memory L2)   │
│   ingest)         │                      └─────────┬─────────┘
└───────────────────┘                                │
        │                                            │ snapshots / deltas
        │                                            ▼
        │                                    ┌───────────────────┐
        │                                    │  Strategy Engine  │
        │                                    │  (signals)        │
        │                                    └─────────┬─────────┘
        │                                              │ signals
        │                                              ▼
        │                                    ┌───────────────────┐
        │                                    │  Risk Manager     │
        │                                    │  (limits, checks) │
        │                                    └─────────┬─────────┘
        │                                              │ approved orders
        │                                              ▼
        │                                    ┌───────────────────┐
        └──────────────────────────────────▶│  Execution Engine │
                                             │  (order lifecycle)│
                                             └─────────┬─────────┘
                                                       │
                                                       ▼
                                             ┌───────────────────┐
                                             │ Exchange Connectors│
                                             │ (Binance, etc.)   │
                                             └───────────────────┘
```

- **Feed Handler**: Connects to exchange WebSockets, normalizes messages, updates the order book, and publishes events.
- **Order Book**: Single source of truth for bids/asks; lock-free or low-contention structures; broadcasts snapshots/deltas.
- **Strategy Engine**: Consumes market data and order book, runs strategies, emits trade signals.
- **Risk Manager**: Validates signals against position limits, exposure, and rules; forwards approved orders.
- **Execution Engine**: Maps approved orders to exchange API calls, tracks state (submit/cancel/update/fill), updates positions.
- **Exchange Connectors**: Exchange-specific WebSocket/REST (Binance first); abstracted behind a common trait.

## Folder Structure

```
hft/
├── Cargo.toml                    # Workspace definition
├── ARCHITECTURE.md               # This file
├── crates/
│   ├── hft-core/                 # Shared types, traits, errors, channel wiring
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs          # Price, Qty, OrderSide, etc.
│   │       ├── error.rs
│   │       └── events.rs         # Market/trade/order events
│   │
│   ├── hft-order-book/           # In-memory order book
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── book.rs           # OrderBook struct, levels, depth
│   │
│   ├── hft-exchange/             # Exchange adapters (Binance + trait)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── mod.rs            # ExchangeConnector trait
│   │       └── binance.rs        # Binance WebSocket + REST
│   │
│   ├── hft-feed-handler/         # Market data ingestion
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── handler.rs        # Connects to exchange, updates book, broadcasts
│   │
│   ├── hft-strategy/             # Strategy engine
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs         # Consumes events, runs strategies
│   │       └── strategies/      # Strategy implementations (e.g. stub)
│   │
│   ├── hft-risk/                 # Risk management
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── manager.rs        # Position limits, exposure, order checks
│   │
│   ├── hft-execution/            # Execution engine
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── engine.rs         # Order lifecycle, fills, positions
│   │
│   ├── hft-metrics/              # Latency and throughput metrics
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── metrics.rs        # Latency histograms, counters
│   │
│   ├── hft-logging/              # Persistence and logging
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── persist.rs        # Trade/event persistence
│   │
│   ├── hft-backtesting/          # Backtesting
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── runner.rs         # Replay feed, run strategy, record results
│   │
│   ├── hft-ui/                   # Terminal UI (ratatui)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── app.rs            # App state, layout
│   │       ├── widgets/         # Order book, trades, PnL, logs, etc.
│   │       └── run.rs            # Event loop, tick
│   │
│   └── hft/                      # Main binary
│       ├── Cargo.toml
│       └── src/
│           └── main.rs           # Wire crates, spawn tasks, run UI
│
```

## Component Responsibilities

| Component           | Responsibility |
|---------------------|----------------|
| **hft-core**        | Domain types (Price, Qty, Side, Symbol), events (BookUpdate, Trade, OrderEvent), errors, and shared channel/bus types. |
| **hft-order-book**  | Central L2 order book: apply incremental updates, maintain bids/asks, support snapshots and deltas; designed for low contention. |
| **hft-exchange**    | `ExchangeConnector` trait; Binance WebSocket (depth/trades) and REST (orders); placeholders for other exchanges. |
| **hft-feed-handler**| Subscribe to exchange streams, normalize to core types, apply updates to order book, broadcast to strategy and UI. |
| **hft-strategy**    | Subscribe to feed-handler/book events; run one or more strategy modules; emit signals (e.g. buy/sell, size, limit price). |
| **hft-risk**        | Validate signals: position limits, max exposure, order size/price sanity; approve or reject before execution. |
| **hft-execution**   | Turn approved signals into orders; submit/cancel/update via exchange connector; track fills and maintain position state. |
| **hft-metrics**     | Measure latency (e.g. feed → strategy → execution), throughput, and expose for UI and monitoring. |
| **hft-logging**     | Persist trades, order events, and key state changes for audit and analysis. |
| **hft-backtesting** | Load historical data, replay through feed-handler + order book, run strategy (and optionally risk/execution stubs), record PnL and stats. |
| **hft-ui**          | TUI (ratatui): order book, recent trades, charts, positions, PnL, win rate, cumulative P&L, latency metrics, system logs. |

## Concurrency and Latency

- **Tokio**: All I/O and async tasks run on Tokio; avoid blocking in hot paths.
- **Channels**: Use `tokio::sync::broadcast` or `mpsc` for feed → book → strategy → risk → execution; bounded to apply backpressure.
- **Order book**: Prefer a single-writer (feed-handler) model with lock-free or atomic reads for snapshotting; or a copy-on-write snapshot for subscribers.
- **No shared mutable state across components**: Each component owns its state and receives input via channels; reduces lock contention.

## Extensibility

- **New exchanges**: Implement `ExchangeConnector` in `hft-exchange`; feed-handler uses a connector trait so one code path supports all exchanges.
- **New strategies**: Implement a common `Strategy` trait in `hft-strategy` and register with the engine; engine forwards market events and collects signals.
- **New UI panels**: Add widgets in `hft-ui/widgets` and subscribe to the same events/snapshots the strategy uses.

## Technology Choices

| Area           | Choice        | Rationale |
|----------------|---------------|-----------|
| Runtime        | Tokio         | Async I/O, low overhead, ecosystem. |
| TUI            | ratatui       | Active fork of tui-rs, terminal UI. |
| WebSocket      | tokio-tungstenite | Async WebSocket with Tokio. |
| Serialization  | serde + serde_json | Exchange APIs are JSON. |
| Order book     | Custom or rustc_hash + BTreeMap | Fast, deterministic depth. |

---

This document defines the intended architecture; the code in each crate provides a **scalable foundation** with clear boundaries and stub implementations where full trading logic will be added later.
