# Strategy Research and Optimization Framework

This document describes the **strategy research and optimization** components added to the HFT system. For full architecture and integration details, see [STRATEGY_RESEARCH_ARCHITECTURE.md](STRATEGY_RESEARCH_ARCHITECTURE.md).

## New Crates

| Crate | Purpose |
|-------|--------|
| **hft-performance-metrics** | PnL, win rate, Sharpe ratio, max drawdown, avg trade duration, trade frequency |
| **hft-optimizer** | Parameter space (grid / random search), run backtests per param set, rank results |
| **hft-experiment-runner** | Run multiple strategies × params, save to JSONL or memory, rank by metric |
| **hft-data-recorder** | Record FeedEvent stream to JSONL; replay for backtests |

## Extended Crates

- **hft-strategy**: Added `SignalIntent` (Buy/Sell/Hold), optional `on_orderbook_update`, `on_trade`, `on_ticker_update` with defaults that delegate to `on_feed_event`, and `order_request()` helper.
- **hft-backtesting**: Replay stream of `ReplayEvent`, run strategies, simulate execution (fill at mid), optional speed multiplier, output `BacktestResult` with `PerformanceReport` and list of trades.
- **hft-feed-handler**: `FeedEvent` and `FeedHandler` implemented in `handler.rs`; `FeedEvent` is an enum (OrderBookSnapshot, OrderBookDelta, Trade, Ticker) and is serializable for recording.
- **hft-ui**: Strategy comparison widget and `App.strategy_comparison` / `StrategyComparisonLine` for displaying backtest/optimization results (PnL, win %, drawdown, Sharpe, best params).

## Quick Start

### 1. Strategy abstraction

Implement the `Strategy` trait and override the callbacks you need:

```rust
use hft_strategy::{Strategy, Signal, SignalIntent, order_request};
use hft_feed_handler::FeedEvent;

struct MyStrategy { /* params */ }

impl Strategy for MyStrategy {
    fn name(&self) -> &str { "my_strategy" }

    fn on_orderbook_update(&self, event: &FeedEvent, signal_tx: &mpsc::Sender<Signal>) {
        // BUY/SELL → send Signal with OrderRequest; HOLD → send nothing
        let req = order_request("BTCUSDT", OrderSide::Buy, qty, Some(price));
        let _ = signal_tx.try_send(Signal { request: req, strategy_id: self.name().into(), generated_at: Instant::now() });
    }
}
```

### 2. Backtesting

Build a list of `ReplayEvent` (from file via `hft_data_recorder::ReplayReader` or in-memory), create a `BacktestRunner` with `BacktestConfig`, and run:

```rust
use hft_backtesting::{BacktestRunner, BacktestConfig, ReplayEvent};

let config = BacktestConfig { symbol: "BTCUSDT".into(), speed_multiplier: 0.0, ..Default::default() };
let order_book = Arc::new(OrderBook::new(config.symbol.clone()));
let runner = BacktestRunner::new(config, order_book);
let result = runner.run(events, &[strategy]).await;
// result.performance (PerformanceReport), result.trades
```

### 3. Parameter optimization

Define a `ParameterSpace`, use `GridSearch` or `RandomSearch`, and run a backtest for each point:

```rust
use hft_optimizer::{ParameterSpace, GridSearch, OptimizationRunner, OptimizationResult};

let space = ParameterSpace::new()
    .add_float("spread_threshold", vec![0.001, 0.002, 0.005])
    .add_int("order_size", vec![1, 2, 5]);
let grid = GridSearch::new(space);
for params in grid {
    // Build strategy with params, run backtest, collect OptimizationResult
}
let ranked = OptimizationRunner::rank_by_sharpe(results);
```

### 4. Experiments and storage

Use `ExperimentRunner` with `JsonlFileStore` or `MemoryStore` to save each run (strategy name, params, metrics, timestamp), then rank:

```rust
use hft_experiment_runner::{ExperimentRunner, JsonlFileStore, ExperimentRecord};

let store = JsonlFileStore::new("experiments.jsonl");
let runner = ExperimentRunner::new(store);
runner.save_result("my_strategy", optimization_result, run_id)?;
// Later: load JSONL, parse into ExperimentRecord, push to App.strategy_comparison for UI
```

### 5. Recording and replaying data

Record live or simulated feed to a file for reproducible backtests:

```rust
use hft_data_recorder::{RecordWriter, ReplayReader};

let mut writer = RecordWriter::new("feed.jsonl")?;
writer.write(ts, &feed_event)?;

// Replay
let reader = ReplayReader::open("feed.jsonl")?;
let events: Vec<_> = reader.into_events().filter_map(Result::ok).collect();
```

### 6. Strategy comparison in the UI

Populate `App.strategy_comparison` with `StrategyComparisonLine` (name, pnl, win_rate_pct, max_drawdown_pct, sharpe, best_params). The TUI shows a "Strategy Comparison" panel at the bottom with current PnL, win rate, drawdown, and best parameter sets.

## Design principles

- **Modular**: Strategies depend only on feed types; backtester and optimizer depend on strategy and metrics.
- **Reproducible**: Experiments store params + metrics + timestamp; data recorder allows replaying the same feed.
- **Extensible**: Add new strategies in `hft-strategy/strategies/`; add new parameter dimensions in the optimizer; add new metrics in `hft-performance-metrics` without changing the rest of the pipeline.
