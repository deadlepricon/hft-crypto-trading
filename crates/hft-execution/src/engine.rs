//! Execution engine: order lifecycle and fill handling.
//!
//! Receives [OrderWithStrategy] from risk manager so fills can be attributed. In **live** mode
//! calls the exchange connector; in **paper** mode simulates an immediate fill at mid and sends
//! [PaperFill] to the UI and [StrategyFill] to the strategy engine (by strategy_id).

use hft_core::{OrderSide, OrderType};
use hft_exchange::ExchangeConnector;
use hft_order_book::OrderBook;
use hft_strategy::{OrderAck, OrderWithStrategy, StrategyFill};
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
        submit_connector: Option<Arc<dyn ExchangeConnector>>,
    },
}

/// One simulated fill: the order, fill price, and computed PnL (for UI).
#[derive(Debug, Clone)]
pub struct PaperFill {
    pub request: hft_core::OrderRequest,
    pub fill_price: Decimal,
    pub pnl_delta: f64,
    pub is_buy: bool,
    pub unrealized_pnl: f64,
    pub qty_after: f64,
    pub entry_price_after: f64,
}

/// Cancel event emitted when execution successfully cancels a resting order.
/// Consumed by the TUI to show the cancels panel and update cancel counters.
#[derive(Debug, Clone)]
pub struct CancelEvent {
    pub timestamp: std::time::Instant,
    pub order_id: String,
    pub symbol: String,
    pub side: hft_core::OrderSide,
    /// Original quote price of the cancelled order.
    pub original_quote_price: Decimal,
    /// Mid price at the moment of cancellation.
    pub price_at_cancel: Decimal,
    /// Reason the strategy cancelled this order (e.g. "DRIFT+AGE", "INVENTORY", "MAX_AGE").
    pub cancel_reason: String,
}

/// Called by paper execution to compute PnL and update internal position state.
pub trait PositionTracker: Send + Sync {
    fn apply_fill(&self, req: &hft_core::OrderRequest, fill_price: Decimal) -> (f64, bool, f64, f64, f64);
}

pub struct ExecutionEngine {
    mode: ExecutionMode,
    order_rx: mpsc::Receiver<OrderWithStrategy>,
    risk_position_tx: Option<mpsc::Sender<(String, f64)>>,
    /// Send order acks back to strategy engine so strategies can track open order IDs.
    order_ack_tx: Option<mpsc::UnboundedSender<OrderAck>>,
    /// Send cancel events to the TUI.
    cancel_tx: Option<mpsc::UnboundedSender<CancelEvent>>,
}

impl ExecutionEngine {
    pub fn new_live(
        connector: Arc<dyn ExchangeConnector>,
        order_rx: mpsc::Receiver<OrderWithStrategy>,
    ) -> Self {
        Self {
            mode: ExecutionMode::Live(connector),
            order_rx,
            risk_position_tx: None,
            order_ack_tx: None,
            cancel_tx: None,
        }
    }

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
            order_ack_tx: None,
            cancel_tx: None,
        }
    }

    pub fn set_risk_position_tx(&mut self, tx: mpsc::Sender<(String, f64)>) {
        self.risk_position_tx = Some(tx);
    }

    pub fn set_order_ack_tx(&mut self, tx: mpsc::UnboundedSender<OrderAck>) {
        self.order_ack_tx = Some(tx);
    }

    pub fn set_cancel_tx(&mut self, tx: mpsc::UnboundedSender<CancelEvent>) {
        self.cancel_tx = Some(tx);
    }

    pub async fn run(&mut self) -> hft_core::Result<()> {
        while let Some(OrderWithStrategy { request: req, strategy_id }) = self.order_rx.recv().await {
            // Handle cancel orders specially: call cancel_order on connector, emit CancelEvent.
            if req.order_type == OrderType::Cancel {
                let order_id_to_cancel = match req.client_order_id.as_deref() {
                    Some(id) => id.to_string(),
                    None => {
                        warn!(symbol = %req.symbol, "cancel signal missing order_id in client_order_id; skipping");
                        continue;
                    }
                };
                let original_price = req.price.unwrap_or(Decimal::ZERO);

                match &self.mode {
                    ExecutionMode::Live(connector) => {
                        match connector.cancel_order(&req.symbol, &order_id_to_cancel).await {
                            Ok(()) => {
                                info!(order_id = %order_id_to_cancel, symbol = %req.symbol, "cancel submitted (live)");
                            }
                            Err(e) => {
                                warn!(error = %e, order_id = %order_id_to_cancel, "cancel_order failed (live)");
                            }
                        }
                    }
                    ExecutionMode::Paper { order_book, submit_connector, .. } => {
                        // Prefer the mid recorded by the strategy at evaluation time so the
                        // display reflects what the strategy actually saw, not the book state
                        // milliseconds later when execution processes the cancel.
                        let price_at_cancel = req.cancel_eval_mid
                            .unwrap_or_else(|| order_book.mid_price().unwrap_or(Decimal::ZERO));
                        let reason = req.cancel_reason.clone().unwrap_or_else(|| "UNKNOWN".to_string());
                        if let Some(connector) = submit_connector.as_ref() {
                            match connector.cancel_order(&req.symbol, &order_id_to_cancel).await {
                                Ok(()) => {
                                    info!(order_id = %order_id_to_cancel, symbol = %req.symbol, "cancel submitted to simulator");
                                    if let Some(ref tx) = self.cancel_tx {
                                        let _ = tx.send(CancelEvent {
                                            timestamp: std::time::Instant::now(),
                                            order_id: order_id_to_cancel.clone(),
                                            symbol: req.symbol.clone(),
                                            side: req.side,
                                            original_quote_price: original_price,
                                            price_at_cancel,
                                            cancel_reason: reason,
                                        });
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, order_id = %order_id_to_cancel, "cancel_order failed (paper/simulator)");
                                }
                            }
                        } else {
                            // No connector: just emit cancel event for UI (no order to cancel in market)
                            if let Some(ref tx) = self.cancel_tx {
                                let _ = tx.send(CancelEvent {
                                    timestamp: std::time::Instant::now(),
                                    order_id: order_id_to_cancel,
                                    symbol: req.symbol.clone(),
                                    side: req.side,
                                    original_quote_price: original_price,
                                    price_at_cancel,
                                    cancel_reason: reason,
                                });
                            }
                        }
                    }
                }
                continue;
            }

            // Normal order submission
            match &self.mode {
                ExecutionMode::Live(connector) => {
                    match connector.submit_order(req.clone()).await {
                        Ok(order_id) => {
                            info!(order_id = %order_id, symbol = %req.symbol, "order submitted");
                            if let Some(ref tx) = self.order_ack_tx {
                                let _ = tx.send(OrderAck {
                                    order_id,
                                    strategy_id: strategy_id.clone(),
                                    symbol: req.symbol.clone(),
                                    side: req.side,
                                    price: req.price,
                                });
                            }
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
                            Ok(order_id) => {
                                tracing::debug!(symbol = %req.symbol, order_id = %order_id, "paper order submitted to simulator");
                                if let Some(ref tx) = self.order_ack_tx {
                                    let _ = tx.send(OrderAck {
                                        order_id,
                                        strategy_id: strategy_id.clone(),
                                        symbol: req.symbol.clone(),
                                        side: req.side,
                                        price: req.price,
                                    });
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, symbol = %req.symbol, "paper submit to market failed");
                            }
                        }
                        // Fills come from simulator WebSocket, not simulated here.
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
                                order_id: None,
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
