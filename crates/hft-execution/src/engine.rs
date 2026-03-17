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
/// Paper mode can optionally submit orders to a connector (e.g. simulator) so the market logs them.
pub enum ExecutionMode {
    Live(Arc<dyn ExchangeConnector>),
    Paper {
        order_book: Arc<OrderBook>,
        fill_tx: mpsc::UnboundedSender<PaperFill>,
        strategy_fill_tx: Option<mpsc::UnboundedSender<StrategyFill>>,
        position_tracker: Arc<dyn PositionTracker>,
        /// When set (e.g. simulator), submit each order to the connector so the market logs it; we still simulate fill locally.
        submit_connector: Option<Arc<dyn ExchangeConnector>>,
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
    /// Net position size after this fill (positive=long, negative=short, 0=flat).
    pub qty_after: f64,
    /// Weighted-average entry price of the position after this fill.
    pub entry_price_after: f64,
}

/// Called by paper execution to compute PnL and update internal position state.
pub trait PositionTracker: Send + Sync {
    /// Returns (realized_pnl_delta, is_buy, unrealized_pnl_after_fill, qty_after, entry_price_after).
    fn apply_fill(&self, req: &hft_core::OrderRequest, fill_price: Decimal) -> (f64, bool, f64, f64, f64);
}

/// Execution engine: submits orders (live or paper) and reports fills.
pub struct ExecutionEngine {
    mode: ExecutionMode,
    order_rx: mpsc::Receiver<OrderWithStrategy>,
    risk_position_tx: Option<mpsc::Sender<(String, f64)>>,
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
    /// If `submit_connector` is Some (e.g. simulator), orders are also POSTed to that connector so the market can log them.
    pub fn new_paper(
        order_book: Arc<OrderBook>,
        order_rx: mpsc::Receiver<OrderWithStrategy>,
        fill_tx: mpsc::UnboundedSender<PaperFill>,
        strategy_fill_tx: Option<mpsc::UnboundedSender<StrategyFill>>,
        position_tracker: Arc<dyn PositionTracker>,
        submit_connector: Option<Arc<dyn ExchangeConnector>>,
    ) -> Self {
        Self {
            mode: ExecutionMode::Paper {
                order_book,
                fill_tx,
                strategy_fill_tx,
                position_tracker,
                submit_connector,
            },
            order_rx,
            risk_position_tx: None,
        }
    }

    /// Set channel to push position deltas to the risk manager.
    pub fn set_risk_position_tx(&mut self, tx: mpsc::Sender<(String, f64)>) {
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
                    submit_connector,
                } => {
                    if let Some(connector) = submit_connector.as_ref() {
                        let mut req_with_id = req.clone();
                        req_with_id.client_order_id = Some(strategy_id.clone());
                        match connector.submit_order(req_with_id).await {
                            Ok(_order_id) => {
                                tracing::debug!(symbol = %req.symbol, "paper order submitted to simulator");
                            }
                            Err(e) => {
                                warn!(error = %e, symbol = %req.symbol, "paper submit to market failed");
                            }
                        }
                        // Fills come from simulator WebSocket (channel "orders" type "fill"), not simulated here.
                    } else {
                        let fill_price = order_book
                            .mid_price()
                            .or_else(|| order_book.best_bid())
                            .or_else(|| order_book.best_ask())
                            .unwrap_or(Decimal::ZERO);
                        let (pnl_delta, is_buy, unrealized_pnl, qty_after, entry_price_after) =
                            position_tracker.apply_fill(&req, fill_price);
                        if fill_tx.send(PaperFill {
                            request: req.clone(),
                            fill_price,
                            pnl_delta,
                            is_buy,
                            unrealized_pnl,
                            qty_after,
                            entry_price_after,
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
                            let qty_f: f64 = req.qty.to_string().parse().unwrap_or(0.0);
                            let delta = if matches!(req.side, OrderSide::Buy) { qty_f } else { -qty_f };
                            let _ = tx.send((req.symbol.clone(), delta)).await;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
