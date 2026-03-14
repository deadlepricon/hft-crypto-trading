//! Execution engine: order lifecycle and fill handling.
//!
//! Receives [OrderWithStrategy] from risk manager so fills can be attributed. In **live** mode
//! calls the exchange connector; in **paper** mode simulates an immediate fill at mid and sends
//! [PaperFill] to the UI and [StrategyFill] to the strategy engine (by strategy_id).

use hft_core::OrderSide;
use hft_exchange::ExchangeConnector;
use hft_order_book::OrderBook;
use hft_strategy::{OrderWithStrategy, StrategyFill};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Execution mode: live (real exchange) or paper (simulated fills).
pub enum ExecutionMode {
    Live(Arc<dyn ExchangeConnector>),
    Paper {
        order_book: Arc<OrderBook>,
        fill_tx: mpsc::UnboundedSender<PaperFill>,
        strategy_fill_tx: Option<mpsc::UnboundedSender<StrategyFill>>,
        position_tracker: Arc<dyn PositionTracker>,
    },
}

/// One simulated fill: the order, fill price, and computed PnL (for UI).
#[derive(Debug, Clone)]
pub struct PaperFill {
    pub request: hft_core::OrderRequest,
    pub fill_price: Decimal,
    /// Realized PnL from this fill (non-zero only when reducing/closing position).
    pub pnl_delta: f64,
    pub is_buy: bool,
    /// Unrealized PnL after this fill (position * (mark - entry)) so UI can show live PnL.
    pub unrealized_pnl: f64,
}

/// Called by paper execution to compute PnL and update internal position state.
pub trait PositionTracker: Send + Sync {
    /// Returns (realized_pnl_delta, is_buy, unrealized_pnl_after_fill).
    fn apply_fill(&self, req: &hft_core::OrderRequest, fill_price: Decimal) -> (f64, bool, f64);
}

/// Execution engine: submits orders (live or paper) and reports fills.
pub struct ExecutionEngine {
    mode: ExecutionMode,
    order_rx: mpsc::Receiver<OrderWithStrategy>,
    risk_position_tx: Option<mpsc::Sender<(String, i64)>>,
}

impl ExecutionEngine {
    /// Live mode: submit orders via the exchange connector.
    pub fn new_live(
        connector: Arc<dyn ExchangeConnector>,
        order_rx: mpsc::Receiver<OrderWithStrategy>,
    ) -> Self {
        Self {
            mode: ExecutionMode::Live(connector),
            order_rx,
            risk_position_tx: None,
        }
    }

    /// Paper mode: simulate immediate fill at order book mid, compute PnL via tracker,
    /// send [PaperFill] to UI and optionally [StrategyFill] to strategy engine.
    pub fn new_paper(
        order_book: Arc<OrderBook>,
        order_rx: mpsc::Receiver<OrderWithStrategy>,
        fill_tx: mpsc::UnboundedSender<PaperFill>,
        strategy_fill_tx: Option<mpsc::UnboundedSender<StrategyFill>>,
        position_tracker: Arc<dyn PositionTracker>,
    ) -> Self {
        Self {
            mode: ExecutionMode::Paper {
                order_book,
                fill_tx,
                strategy_fill_tx,
                position_tracker,
            },
            order_rx,
            risk_position_tx: None,
        }
    }

    /// Set channel to push position deltas to the risk manager.
    pub fn set_risk_position_tx(&mut self, tx: mpsc::Sender<(String, i64)>) {
        self.risk_position_tx = Some(tx);
    }

    /// Run: receive orders, execute (live or paper), report fills in paper mode.
    pub async fn run(&mut self) -> hft_core::Result<()> {
        while let Some(OrderWithStrategy { request: req, strategy_id }) = self.order_rx.recv().await {
            match &self.mode {
                ExecutionMode::Live(connector) => {
                    match connector.submit_order(req.clone()).await {
                        Ok(order_id) => {
                            info!(order_id = %order_id, symbol = %req.symbol, "order submitted");
                        }
                        Err(e) => {
                            warn!(error = %e, symbol = %req.symbol, "order submit failed");
                        }
                    }
                }
                ExecutionMode::Paper {
                    order_book,
                    fill_tx,
                    strategy_fill_tx,
                    position_tracker,
                } => {
                    let fill_price = order_book
                        .mid_price()
                        .or_else(|| order_book.best_bid())
                        .or_else(|| order_book.best_ask())
                        .unwrap_or(Decimal::ZERO);
                    let (pnl_delta, is_buy, unrealized_pnl) = position_tracker.apply_fill(&req, fill_price);
                    if fill_tx.send(PaperFill {
                        request: req.clone(),
                        fill_price,
                        pnl_delta,
                        is_buy,
                        unrealized_pnl,
                    }).is_err() {
                        break;
                    }
                    if let Some(tx) = strategy_fill_tx.as_ref() {
                        let _ = tx.send(StrategyFill {
                            strategy_id: strategy_id.clone(),
                            symbol: req.symbol.clone(),
                            side: req.side,
                            filled_qty: req.qty,
                            fill_price,
                        });
                    }
                    if let Some(ref tx) = self.risk_position_tx {
                        let qty: i64 = req.qty.to_string().parse().unwrap_or(0);
                        let delta = if matches!(req.side, OrderSide::Buy) { qty } else { -qty };
                        let _ = tx.send((req.symbol.clone(), delta)).await;
                    }
                }
            }
        }
        Ok(())
    }
}
