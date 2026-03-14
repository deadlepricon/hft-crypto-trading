# Strategy Research and Optimization Framework — Architecture

This document describes how the new strategy research, backtesting, optimization, and experiment components integrate with the existing HFT system.

## Current System Summary

- **Data flow:** Exchange → Connector → FeedHandler → OrderBook + broadcast `EventEnvelope<FeedEvent>` → StrategyEngine → strategies emit `Signal` → Risk → Execution → Connector.
- **Key types:** `FeedEvent` (market data), `Signal` (OrderRequest + strategy_id), `OrderBook`, `OrderBookSnapshot`, `TradeEvent`, `FillEvent`, `OrderRequest`, `Position`.
- **Crates:** hft-core, hft-order-book, hft-exchange, hft-feed-handler, hft-strategy, hft-risk, hft-execution, hft-metrics, hft-logging, hft-backtesting, hft-ui, hft (binary).

## New Components and Integration

### 1. Strategy Abstraction Layer (hft-strategy)

**Location:** Existing `hft-strategy` crate; extend trait and add shared signal types.

**Design:**

- **Unified event:** Strategies continue to receive `FeedEvent`. `FeedEvent` is an enum: `OrderBookSnapshot`, `OrderBookDelta`, `Trade(TradeEvent)`, and optionally `Ticker(TickerUpdate)`. Strategies can implement a single `on_feed_event()` and branch on variant, or we add optional `on_orderbook_update()`, `on_trade()`, `on_ticker_update()` with default implementations that delegate to `on_feed_event()` for backward compatibility.
- **Signal semantics:** BUY/SELL = strategy sends a `Signal` (OrderRequest). HOLD = strategy sends nothing. A small `SignalIntent` enum (Buy / Sell / Hold) can be used inside strategies for clarity before converting to `OrderRequest` when not Hold.
- **Multiple strategies:** `StrategyEngine` already holds `Vec<Arc<dyn Strategy>>` and runs each on every event; no change.

**Integration:** Feed handler already broadcasts `EventEnvelope<FeedEvent>`. Strategy engine already consumes that and forwards to strategies. Add `TickerUpdate` to hft-core events and `FeedEvent::Ticker` when ticker data is available.

---

### 2. Backtesting Engine (hft-backtesting)

**Location:** Extend existing `hft-backtesting` crate.

**Responsibilities:**

- **Data source:** Consume a stream of timestamped market events (e.g. `(DateTime<Utc>, FeedEvent)` or a recorded file produced by the data recorder). Replay in chronological order.
- **Order book:** Apply snapshots/deltas to the same `OrderBook` type used in live trading so strategy logic sees identical state.
- **Strategy dispatch:** For each replayed event, build `EventEnvelope<FeedEvent>` and call each strategy’s `on_feed_event()` (or run the existing `StrategyEngine` with a channel fed by the replayer).
- **Execution simulation:** Intercept signals; simulate fills (e.g. at mid, or at next trade price, or spread-based). Produce `FillEvent`s and maintain a simulated position ledger.
- **Speed:** Replay with a configurable speed multiplier (e.g. no sleep, or `event_dt / speed_factor`) so backtests can run faster than real time.
- **Output:** Time series of fills and positions, plus a summary struct (for the performance metrics module).

**Integration:** Uses `hft-order-book`, `hft-feed-handler` (FeedEvent), `hft-strategy` (Strategy, Signal), `hft-core` (events, types). Does not use the live `ExchangeConnector`; instead uses an internal execution simulator that implements the same “submit → fill” contract.

---

### 3. Performance Evaluation Metrics (hft-performance-metrics)

**Location:** New crate `hft-performance-metrics`.

**Responsibilities:**

- **Inputs:** List of simulated or live trades (entry/exit price, qty, side, timestamps) and/or an equity curve (timestamp, cumulative PnL).
- **Metrics:** Total PnL, win rate, number of wins/losses, Sharpe ratio (configurable risk-free rate and period), maximum drawdown, average trade duration, trade frequency (trades per unit time). All computed after each backtest (or live window).
- **Output:** A single struct (e.g. `PerformanceReport`) consumed by the backtester (to attach to `BacktestResult`), the optimizer, and the UI.

**Integration:** Pure functions or a small struct that takes trade list / equity curve and returns metrics. No I/O. Used by hft-backtesting, hft-optimizer, hft-experiment-runner, and hft-ui.

---

### 4. Strategy Optimization Framework (hft-optimizer)

**Location:** New crate `hft-optimizer`.

**Responsibilities:**

- **Parameter space:** Define continuous or discrete parameters (e.g. spread threshold, order size, imbalance threshold). Support:
  - **Grid search:** Cartesian product of discrete values.
  - **Random search:** Sample N points (uniform or from distributions).
  - **Bayesian optimization:** Optional; use a crate (e.g. for a surrogate model) to suggest next parameter set from previous backtest results.
- **Runner:** For each parameter set, build strategy (or clone with params), run backtest, collect `BacktestResult` + `PerformanceReport`, optionally persist.
- **Output:** Ranked list of (parameters, metrics) for the experiment runner and UI.

**Integration:** Depends on hft-backtesting, hft-strategy, hft-performance-metrics, hft-core. Strategies must be constructible with a parameter struct (e.g. `StrategyParams`) so the optimizer can instantiate them per run.

---

### 5. Experiment Runner (hft-experiment-runner)

**Location:** New crate `hft-experiment-runner`.

**Responsibilities:**

- **Orchestration:** Run multiple strategies × multiple parameter configurations. For each (strategy, params): run backtest, compute metrics, store result.
- **Storage:** Each experiment record: strategy name, parameter set (serialized), performance metrics, timestamp, run id. Stored in a simple format (e.g. JSON/JSONL or SQLite) for reproducibility and later analysis.
- **Ranking:** Sort by a chosen metric (e.g. Sharpe, PnL, or win rate) and expose best-performing strategy/parameter sets to the UI and to downstream tooling.

**Integration:** Depends on hft-backtesting, hft-optimizer (or uses backtester + parameter generator directly), hft-performance-metrics, hft-logging. Can depend on a small “experiment store” abstraction (file or DB) that the data recorder can share.

---

### 6. Logging and Data Storage (hft-logging + data recorder)

**Existing:** `hft-logging` for application logs.

**New / extended:**

- **Experiment results:** Stored by the experiment runner (see above). Schema: run_id, strategy_name, params (JSON), metrics (JSON), timestamp.
- **Data recorder (optional new crate or module):** Record `FeedEvent` stream (e.g. from feed handler) to disk (JSONL or binary). Backtester can replay from these files for reproducible backtests. This completes the loop: live/recorded data → file → backtest → metrics → storage.

**Integration:** Experiment runner writes to the chosen store. Backtester reads from files or in-memory streams. Same `FeedEvent` format everywhere.

---

### 7. Strategy Comparison Dashboard (hft-ui)

**Location:** Extend existing `hft-ui` crate.

**Responsibilities:**

- **State:** Extend `App` (or a dedicated research state) with: list of experiment results (strategy name, PnL, win rate, drawdown, best params). Can be filled from the experiment runner output or by loading stored results.
- **Widget:** New widget (e.g. “Strategy comparison” or “Experiments”): table or list showing current PnL, win rate, drawdown, best-performing parameter sets. Optional: filter by strategy, sort by metric.
- **Modes:** Live trading view unchanged; when in “research” or “experiment” mode, show the strategy comparison view (and optionally trigger runs from CLI, not necessarily from the UI).

**Integration:** App holds optional `Vec<ExperimentSummary>`. Widget reads from it. Experiment runner can write results to a file that the UI periodically loads, or the UI can be driven by a CLI that runs experiments and then launches the UI with result path.

---

## Module Layout (Crates)

| Crate                    | Role                                                                 |
|--------------------------|----------------------------------------------------------------------|
| **hft-core**             | Add `TickerUpdate` if needed; optional re-export of metrics types.   |
| **hft-feed-handler**     | Define `FeedEvent` (OrderBookSnapshot, OrderBookDelta, Trade, Ticker). |
| **hft-strategy**         | Strategy trait; optional `on_orderbook_update`/`on_trade`/`on_ticker`; SignalIntent (Buy/Sell/Hold) for clarity. |
| **hft-backtesting**      | Replay loop, execution simulator, position tracking, speed control, output for metrics. |
| **hft-performance-metrics** | New: PnL, Sharpe, drawdown, win rate, trade duration, frequency.  |
| **hft-optimizer**        | New: grid/random/Bayesian parameter search; run backtests; return ranked (params, metrics). |
| **hft-experiment-runner** | New: multi-strategy × params; store results; rank.                   |
| **hft-data-recorder**    | New (optional): record FeedEvent stream to file; replay API for backtester. |
| **hft-ui**               | Strategy comparison widget; optional research mode.                 |

## Data Flow (Research Path)

1. **Record (optional):** Live feed → FeedHandler → DataRecorder → file.
2. **Backtest:** File or in-memory stream → BacktestRunner (replay + execution sim) → StrategyEngine / strategies → signals → simulated fills → positions + trade list.
3. **Metrics:** Trade list + equity curve → PerformanceMetrics → `PerformanceReport`.
4. **Optimize:** Parameter generator (grid/random/Bayesian) → for each params: backtest → metrics → collect; rank.
5. **Experiment:** For each (strategy, param set): backtest → metrics → store; rank; optionally feed to UI.
6. **UI:** Load or receive experiment results → Strategy comparison dashboard (PnL, win rate, drawdown, best params).

## Clean Separation

- **Strategies** are independent modules implementing the same `Strategy` trait; they receive `FeedEvent` and optionally send `Signal`. They do not depend on backtesting or optimization.
- **Backtester** depends on strategy and feed types but does not know about optimization or experiments.
- **Optimizer** and **experiment runner** depend on backtester and metrics; they are the only ones that run many backtests and persist results.
- **UI** only displays data; it can read stored experiment results or receive them from the runner.

This keeps the system modular and makes it easy to add new strategies and new parameter spaces without touching the infrastructure.
