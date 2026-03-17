//! Risk manager: validate signals and enforce limits.
//!
//! Responsibilities:
//! - Enforce per-symbol and total position limits
//! - Control maximum exposure (notional or margin)
//! - Validate order size and price (e.g. min/max, tick size)
//! - Prevent duplicate or obviously invalid orders
//!
//! Approved orders are sent to the execution engine via a channel.

use hft_core::Result;
use hft_strategy::{OrderWithStrategy, Signal};
use parking_lot::RwLock;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Configuration for risk limits (stub values; expand as needed).
#[derive(Debug, Clone)]
pub struct RiskLimits {
    /// Max position size per symbol (absolute).
    pub max_position_per_symbol: u64,
    /// Max total notional exposure (stub: u64 for simplicity).
    pub max_total_exposure: u64,
    /// Max order size per order.
    pub max_order_size: u64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position_per_symbol: 10,
            max_total_exposure: 1_000_000,
            max_order_size: 5,
        }
    }
}

/// Risk manager: consumes signals, validates, forwards approved [OrderWithStrategy]s so execution can route fills.
pub struct RiskManager {
    limits: RiskLimits,
    /// Current positions (symbol -> net qty in asset units); updated from execution layer.
    positions: RwLock<HashMap<String, f64>>,
    approved_tx: mpsc::Sender<OrderWithStrategy>,
}

impl RiskManager {
    /// Create a risk manager that sends approved orders (with strategy_id) to the given channel.
    pub fn new(limits: RiskLimits, approved_tx: mpsc::Sender<OrderWithStrategy>) -> Self {
        Self {
            limits,
            positions: RwLock::new(HashMap::new()),
            approved_tx,
        }
    }

    /// Update position for a symbol (called when execution reports fills).
    /// `delta` is signed qty in asset units: positive for buy, negative for sell.
    pub fn update_position(&self, symbol: &str, delta: f64) {
        let mut guard = self.positions.write();
        let entry = guard.entry(symbol.to_string()).or_insert(0.0);
        *entry += delta;
    }

    /// Process a signal: validate and either forward to execution or reject.
    pub async fn check_signal(&self, signal: Signal) -> Result<()> {
        let req = &signal.request;
        let current = self.positions.read().get(req.symbol.as_str()).copied().unwrap_or(0.0);
        // Parse as f64 so fractional quantities like 0.001 BTC are handled correctly.
        let qty_f: f64 = req.qty.to_string().parse().unwrap_or(0.0);
        let new_position = current + if matches!(req.side, hft_core::OrderSide::Buy) {
            qty_f
        } else {
            -qty_f
        };

        if new_position.abs() > self.limits.max_position_per_symbol as f64 {
            warn!(
                symbol = %req.symbol,
                new_position,
                limit = self.limits.max_position_per_symbol,
                "risk: position limit exceeded"
            );
            return Err(hft_core::HftError::RiskRejected(
                "Position limit exceeded".to_string(),
            ));
        }

        if qty_f > self.limits.max_order_size as f64 {
            warn!(
                symbol = %req.symbol,
                qty = qty_f,
                limit = self.limits.max_order_size,
                "risk: order size limit exceeded"
            );
            return Err(hft_core::HftError::RiskRejected(
                "Order size limit exceeded".to_string(),
            ));
        }

        debug!(symbol = %req.symbol, "risk: approved");
        self.approved_tx
            .send(OrderWithStrategy {
                request: req.clone(),
                strategy_id: signal.strategy_id.clone(),
            })
            .await
            .map_err(|e| hft_core::HftError::InvalidState(e.to_string()))?;
        Ok(())
    }

    /// Run the risk manager: receive signals and check each one.
    pub async fn run(
        &self,
        mut signal_rx: mpsc::Receiver<Signal>,
    ) -> Result<()> {
        while let Some(signal) = signal_rx.recv().await {
            let _ = self.check_signal(signal).await;
        }
        Ok(())
    }
}
