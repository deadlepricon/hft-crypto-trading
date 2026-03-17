//! Binance.US Spot WebSocket connector.
//!
//! Connects to stream.binance.us for depth@100ms and aggTrade (US-compliant).
//! Override with BINANCE_WS_URL env for a full custom URL, or BINANCE_WS_BASE for a different host (e.g. stream.binance.com).

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hft_core::{
    events::{OrderBookDelta, OrderBookSnapshot, TradeEvent},
    OrderRequest, OrderSide, Result, TradeId,
    types::Level,
};
use rust_decimal::Decimal;
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::connector::{ExchangeConnector, ExchangeMessage};

/// Default WebSocket base for Binance.US (US users).
const BINANCE_US_WS_BASE: &str = "wss://stream.binance.us:9443";

/// Binance Spot WebSocket: depth + aggTrade for one symbol.
pub struct BinanceConnector {
    name: String,
    symbol: String,
    ws_url: Option<String>,
}

fn str_to_decimal(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

impl BinanceConnector {
    /// Create connector with default Binance combined stream (depth@100ms + aggTrade).
    pub fn new(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        Self {
            name: "binance.us".to_string(),
            symbol: symbol.clone(),
            ws_url: None,
        }
    }

    /// Create connector with a custom WebSocket URL (e.g. for proxy or testnet).
    pub fn with_ws_url(symbol: impl Into<String>, ws_url: String) -> Self {
        Self {
            name: "binance.us".to_string(),
            symbol: symbol.into(),
            ws_url: Some(ws_url),
        }
    }

    fn combined_stream_url(symbol: &str) -> String {
        let base = std::env::var("BINANCE_WS_BASE")
            .unwrap_or_else(|_| BINANCE_US_WS_BASE.to_string());
        let base = base.trim_end_matches('/');
        let s = symbol.to_lowercase().replace('/', "");
        format!(
            "{}/stream?streams={}@depth@100ms/{}@aggTrade",
            base, s, s
        )
    }
}

#[derive(serde::Deserialize)]
struct CombinedMessage {
    stream: Option<String>,
    data: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct DepthUpdateData {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "b")]
    bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    asks: Vec<[String; 2]>,
}

#[derive(serde::Deserialize)]
struct AggTradeData {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "a")]
    agg_trade_id: u64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "T")]
    trade_time_ms: i64,
    #[serde(rename = "m")]
    buyer_is_maker: bool,
}

#[async_trait]
impl ExchangeConnector for BinanceConnector {
    fn name(&self) -> &str {
        &self.name
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ExchangeMessage>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let ws_url = self
            .ws_url
            .clone()
            .unwrap_or_else(|| Self::combined_stream_url(&self.symbol));

        let _ = tx.send(ExchangeMessage::Connected);

        tokio::spawn(async move {
            loop {
                match tokio_tungstenite::connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        info!(url = %ws_url, "binance ws connected");
                        let (mut write, mut read) = futures_util::StreamExt::split(ws_stream);
                        let mut seq: u64 = 0;
                        while let Some(msg_result) = read.next().await {
                            match msg_result {
                                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                                    if let Ok(env) = serde_json::from_str::<CombinedMessage>(&text) {
                                        let stream_name = env.stream.as_deref().unwrap_or("");
                                        let data = match env.data {
                                            Some(d) => d,
                                            None => continue,
                                        };
                                        if stream_name.ends_with("@depth@100ms") {
                                            if let Ok(d) = serde_json::from_value::<DepthUpdateData>(data) {
                                                seq = seq.wrapping_add(1);
                                                let bids: Vec<Level> = d
                                                    .bids
                                                    .iter()
                                                    .map(|[p, q]| Level {
                                                        price: str_to_decimal(p),
                                                        qty: str_to_decimal(q),
                                                    })
                                                    .collect();
                                                let asks: Vec<Level> = d
                                                    .asks
                                                    .iter()
                                                    .map(|[p, q]| Level {
                                                        price: str_to_decimal(p),
                                                        qty: str_to_decimal(q),
                                                    })
                                                    .collect();
                                                let delta = OrderBookDelta {
                                                    symbol: d.symbol,
                                                    bids,
                                                    asks,
                                                    sequence: seq,
                                                };
                                                if tx.send(ExchangeMessage::OrderBookDelta(delta)).is_err() {
                                                    break;
                                                }
                                            }
                                        } else if stream_name.ends_with("@aggTrade") {
                                            if let Ok(t) = serde_json::from_value::<AggTradeData>(data) {
                                                let timestamp = chrono::DateTime::from_timestamp_millis(t.trade_time_ms)
                                                    .unwrap_or_else(Utc::now);
                                                let side = if t.buyer_is_maker {
                                                    OrderSide::Sell
                                                } else {
                                                    OrderSide::Buy
                                                };
                                                let ev = TradeEvent {
                                                    symbol: t.symbol,
                                                    trade_id: TradeId(t.agg_trade_id.to_string()),
                                                    price: str_to_decimal(&t.price),
                                                    qty: str_to_decimal(&t.qty),
                                                    side,
                                                    timestamp,
                                                };
                                                if tx.send(ExchangeMessage::Trade(ev)).is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                                    let _ = tx.send(ExchangeMessage::Disconnected {
                                        reason: "server closed".to_string(),
                                    });
                                    break;
                                }
                                Ok(tokio_tungstenite::tungstenite::Message::Ping(data)) => {
                                    let _ = write.send(tokio_tungstenite::tungstenite::Message::Pong(data)).await;
                                }
                                Err(e) => {
                                    let reason = e.to_string();
                                    warn!(error = %reason, "binance ws error");
                                    let _ = tx.send(ExchangeMessage::Disconnected { reason });
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        let reason = e.to_string();
                        error!(error = %reason, "binance ws connect failed");
                        let _ = tx.send(ExchangeMessage::Disconnected { reason });
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });

        Ok(rx)
    }

    // TODO: implement Binance REST order submission (POST /api/v3/order with HMAC-SHA256 signing).
    async fn submit_order(&self, _request: OrderRequest) -> Result<String> {
        Err(hft_core::HftError::Exchange(
            "Binance REST order submission not implemented".to_string(),
        ))
    }

    async fn cancel_order(&self, _symbol: &str, _order_id: &str) -> Result<()> {
        Err(hft_core::HftError::Exchange(
            "Binance REST cancel not implemented".to_string(),
        ))
    }

    async fn fetch_order_book_snapshot(&self, _symbol: &str, _depth: u32) -> Result<Option<OrderBookSnapshot>> {
        Ok(None)
    }
}
