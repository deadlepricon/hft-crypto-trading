//! Strategy registry: create strategies by name for config-driven and multi-strategy setup.
//!
//! Use env **STRATEGIES** (comma-separated) to choose which strategies run; multiple strategies
//! run in parallel (each gets every feed event and its own fill stream via strategy_id).

use hft_order_book::OrderBook;
use std::sync::Arc;

use crate::strategies::{
    ImbalanceParams, ImbalanceStrategy, MarketMakerParams, MarketMakerStrategy, Strategy,
};

/// Create a single strategy by name with default params. Symbol is set where the strategy needs it.
pub fn create_strategy(
    name: &str,
    order_book: Arc<OrderBook>,
    symbol: &str,
) -> Option<Arc<dyn Strategy>> {
    match name.trim().to_lowercase().as_str() {
        "market_maker" => {
            let mut p = MarketMakerParams::default();
            p.symbol = symbol.to_string();
            Some(Arc::new(MarketMakerStrategy::new(order_book, p)))
        }
        "imbalance" => {
            let p = ImbalanceParams::default();
            Some(Arc::new(ImbalanceStrategy::new(
                order_book,
                symbol.to_string(),
                p,
            )))
        }
        _ => None,
    }
}

/// Create multiple strategies by name. Skips unknown names (no error). Same order_book/symbol for all.
/// Returns (strategies, names_created) so you can log which strategies are actually running.
pub fn create_strategies(
    order_book: Arc<OrderBook>,
    symbol: &str,
    names: &[impl AsRef<str>],
) -> (Vec<Arc<dyn Strategy>>, Vec<String>) {
    let mut strategies = Vec::new();
    let mut created = Vec::new();
    for name in names {
        let n = name.as_ref().trim();
        if n.is_empty() {
            continue;
        }
        if let Some(s) = create_strategy(n, Arc::clone(&order_book), symbol) {
            created.push(n.to_lowercase().to_string());
            strategies.push(s);
        }
    }
    (strategies, created)
}

/// Supported strategy names for help / validation (e.g. "market_maker", "imbalance").
pub fn strategy_names() -> &'static [&'static str] {
    &["market_maker", "imbalance"]
}
