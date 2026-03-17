//! Exchange simulator connector.
//!
//! Connects to a local simulator: WebSocket at ws://localhost:8765/ws/feed
//! for market data (book delta, trades, ticker), REST at http://localhost:8765
//! for orders.
//!
//! - **Book**: synthetic (simulator-generated); use for placing/matching orders locally.
//! - **Trades**: mostly live from Binance; you also get trades when your orders fill
//!   (simulator uses trade_id like "trd_42", timestamp may be null).
//! - **Ticker**: from Binance last trade (last_price, volume since startup; best_bid/ask = last_price).
//!
//! Binance trade timestamps are **milliseconds since epoch** (string). Simulator fills may have null.

use async_trait::async_trait;
use chrono::Utc;
use hft_core::{
    events::{FillEvent, OrderBookDelta, OrderBookSnapshot, TradeEvent},
    OrderId, OrderRequest, OrderSide, OrderType, Result, TradeId,
    types::{Level, Qty},
};
use futures_util::StreamExt;
use rust_decimal::Decimal;
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{error, warn};

use super::connector::{ExchangeConnector, ExchangeMessage};

/// Default WebSocket URL for the simulator.
pub const SIMULATOR_WS_URL: &str = "ws://localhost:8765/ws/feed";
/// Default REST base URL for the simulator.
pub const SIMULATOR_BASE_URL: &str = "http://localhost:8765";

/// Simulator exchange connector. Same interface as [BinanceConnector]; use
/// [ExchangeKind] or env to choose at runtime.
pub struct SimulatorConnector {
    name: String,
    base_url: String,
    ws_url: String,
    symbol: String,
    client: reqwest::Client,
}

impl SimulatorConnector {
    /// Create a connector to the simulator with default URLs.
    pub fn new(symbol: impl Into<String>) -> Self {
        Self::with_urls(symbol, SIMULATOR_BASE_URL.to_string(), SIMULATOR_WS_URL.to_string())
    }

    /// Create with custom base URL and WebSocket URL (e.g. for different ports).
    pub fn with_urls(
        symbol: impl Into<String>,
        base_url: String,
        ws_url: String,
    ) -> Self {
        let symbol = symbol.into();
        Self {
            name: "simulator".to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            ws_url,
            symbol: symbol.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
        }
    }
}

fn f64_to_decimal(v: f64) -> Qty {
    Decimal::from_str(&format!("{v}")).unwrap_or(Decimal::ZERO)
}

/// Parse timestamp from JSON: string "1699900123456", number, or null → Option<DateTime<Utc>>.
fn parse_timestamp_ms(v: &Option<serde_json::Value>) -> Option<chrono::DateTime<Utc>> {
    let ms = match v.as_ref()? {
        serde_json::Value::String(s) => s.parse::<i64>().ok()?,
        serde_json::Value::Number(n) => n.as_i64()?,
        _ => return None,
    };
    chrono::DateTime::from_timestamp_millis(ms)
}

#[derive(serde::Deserialize)]
struct WsEnvelope {
    channel: Option<String>,
    #[serde(rename = "type")]
    msg_type: Option<String>,
    data: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct BookLevel {
    price: f64,
    quantity: f64,
}

#[derive(serde::Deserialize)]
struct BookDeltaData {
    symbol: String,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    sequence: u64,
}

/// Parse f64 from JSON Value (number or string).
fn value_to_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[async_trait]
impl ExchangeConnector for SimulatorConnector {
    fn name(&self) -> &str {
        &self.name
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ExchangeMessage>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let ws_url = self.ws_url.clone();
        let _symbol = self.symbol.clone();

        let _ = tx.send(ExchangeMessage::Connected);

        tokio::spawn(async move {
            loop {
                match tokio_tungstenite::connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        let (_write, mut read) = futures_util::StreamExt::split(ws_stream);
                        while let Some(msg_result) = read.next().await {
                            match msg_result {
                                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                                    if let Ok(env) = serde_json::from_str::<WsEnvelope>(&text) {
                                        let channel = env.channel.as_deref().unwrap_or("");
                                        let msg_type = env.msg_type.as_deref().unwrap_or("");
                                        if let Some(data) = env.data {
                                            if channel == "book" && msg_type == "delta" {
                                                if let Ok(d) = serde_json::from_value::<BookDeltaData>(data) {
                                                    let bids: Vec<Level> = d
                                                        .bids
                                                        .into_iter()
                                                        .map(|l| Level {
                                                            price: f64_to_decimal(l.price),
                                                            qty: f64_to_decimal(l.quantity),
                                                        })
                                                        .collect();
                                                    let asks: Vec<Level> = d
                                                        .asks
                                                        .into_iter()
                                                        .map(|l| Level {
                                                            price: f64_to_decimal(l.price),
                                                            qty: f64_to_decimal(l.quantity),
                                                        })
                                                        .collect();
                                                    let delta = OrderBookDelta {
                                                        symbol: d.symbol,
                                                        bids,
                                                        asks,
                                                        sequence: d.sequence,
                                                    };
                                                    if tx.send(ExchangeMessage::OrderBookDelta(delta)).is_err() {
                                                        break;
                                                    }
                                                }
                                            } else if channel == "trades" && msg_type == "trade" {
                                                let data_obj = match data {
                                                    serde_json::Value::Object(o) => o.clone(),
                                                    _ => {
                                                        let _ = tx.send(ExchangeMessage::Debug(
                                                            "trade: data not an object".to_string(),
                                                        ));
                                                        continue;
                                                    }
                                                };
                                                let symbol = data_obj
                                                    .get("symbol")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let trade_id = data_obj
                                                    .get("trade_id")
                                                    .map(|v| match v {
                                                        serde_json::Value::String(s) => s.clone(),
                                                        serde_json::Value::Number(n) => n.to_string(),
                                                        _ => "?".to_string(),
                                                    })
                                                    .unwrap_or_else(|| "?".to_string());
                                                let price = data_obj
                                                    .get("price")
                                                    .and_then(value_to_f64)
                                                    .unwrap_or(0.0);
                                                let quantity = data_obj
                                                    .get("quantity")
                                                    .and_then(value_to_f64)
                                                    .unwrap_or(0.0);
                                                let side_str = data_obj
                                                    .get("side")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("sell");
                                                let side = if side_str.to_lowercase().as_str() == "buy" {
                                                    OrderSide::Buy
                                                } else {
                                                    OrderSide::Sell
                                                };
                                                if symbol.is_empty() || trade_id.is_empty() {
                                                    let snippet = format!(
                                                        "trade? keys={}",
                                                        data_obj.keys().cloned().collect::<Vec<_>>().join(",")
                                                    );
                                                    let _ = tx.send(ExchangeMessage::Debug(snippet));
                                                } else {
                                                    let timestamp = data_obj.get("timestamp").cloned();
                                                    let ts = parse_timestamp_ms(&timestamp).unwrap_or_else(Utc::now);
                                                    let ev = TradeEvent {
                                                        symbol,
                                                        trade_id: TradeId(trade_id),
                                                        price: f64_to_decimal(price),
                                                        qty: f64_to_decimal(quantity),
                                                        side,
                                                        timestamp: ts,
                                                    };
                                                    if tx.send(ExchangeMessage::Trade(ev)).is_err() {
                                                        break;
                                                    }
                                                }
                                            } else if channel == "orders" && msg_type == "fill" {
                                                #[derive(serde::Deserialize)]
                                                struct FillData {
                                                    order_id: String,
                                                    #[serde(default)]
                                                    client_order_id: Option<String>,
                                                    symbol: String,
                                                    side: String,
                                                    price: f64,
                                                    quantity: f64,
                                                    fill_id: String,
                                                    #[serde(default)]
                                                    is_maker: bool,
                                                    timestamp: Option<serde_json::Value>,
                                                }
                                                if let Ok(d) = serde_json::from_value::<FillData>(data) {
                                                    let side = if d.side.to_lowercase().as_str() == "buy" {
                                                        OrderSide::Buy
                                                    } else {
                                                        OrderSide::Sell
                                                    };
                                                    let ts = parse_timestamp_ms(&d.timestamp).unwrap_or_else(Utc::now);
                                                    let fill_ev = FillEvent {
                                                        order_id: OrderId(d.order_id),
                                                        trade_id: TradeId(d.fill_id),
                                                        symbol: d.symbol,
                                                        side,
                                                        price: f64_to_decimal(d.price),
                                                        qty: f64_to_decimal(d.quantity),
                                                        timestamp: ts,
                                                        client_order_id: d.client_order_id,
                                                    };
                                                    if tx.send(ExchangeMessage::Fill(fill_ev)).is_err() {
                                                        break;
                                                    }
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
                                Err(e) => {
                                    let reason = e.to_string();
                                    warn!(error = %reason, "simulator ws error");
                                    let _ = tx.send(ExchangeMessage::Disconnected { reason });
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        let reason = e.to_string();
                        error!(error = %reason, "simulator ws connect failed");
                        let _ = tx.send(ExchangeMessage::Disconnected { reason });
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });

        Ok(rx)
    }

    async fn submit_order(&self, request: OrderRequest) -> Result<String> {
        let side = match request.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };
        let order_type = match request.order_type {
            OrderType::Limit => "limit",
            OrderType::Market => "market",
        };
        let quantity: f64 = request.qty.to_string().parse().unwrap_or(0.0);
        let price = request.price.map(|p| p.to_string().parse::<f64>().unwrap_or(0.0));

        let body = serde_json::json!({
            "symbol": request.symbol,
            "side": side,
            "order_type": order_type,
            "quantity": quantity,
            "price": price,
            "client_order_id": request.client_order_id
        });

        let url = format!("{}/api/orders", self.base_url);
        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e: reqwest::Error| hft_core::HftError::Network(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(hft_core::HftError::Exchange(format!(
                "submit_order {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = res
            .json()
            .await
            .map_err(|e| hft_core::HftError::Serialization(e.to_string()))?;
        let order_id = json
            .get("order_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| hft_core::HftError::Exchange("missing order_id in response".to_string()))?
            .to_string();
        Ok(order_id)
    }

    async fn cancel_order(&self, symbol: &str, order_id: &str) -> Result<()> {
        let body = serde_json::json!({
            "symbol": symbol,
            "order_id": order_id
        });
        let url = format!("{}/api/orders/cancel", self.base_url);
        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e: reqwest::Error| hft_core::HftError::Network(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(hft_core::HftError::Exchange(format!(
                "cancel_order {}: {}",
                status, text
            )));
        }
        Ok(())
    }

    async fn fetch_order_book_snapshot(&self, _symbol: &str, _depth: u32) -> Result<Option<OrderBookSnapshot>> {
        Ok(None)
    }
}
