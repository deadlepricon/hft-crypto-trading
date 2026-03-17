//! Feed handler: normalize exchange messages into FeedEvent and broadcast.
//!
//! Consumes [ExchangeMessage] from the connector, updates the shared [OrderBook],
//! and broadcasts [EventEnvelope]<[FeedEvent]> for the strategy engine and UI.

use chrono::Utc;
use hft_core::events::{EventEnvelope, EventSource, FillEvent, OrderBookDelta, OrderBookSnapshot, TradeEvent};
use hft_order_book::OrderBook;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::debug;

use hft_exchange::ExchangeMessage;

/// Normalized market data event for strategies and backtester.
/// Matches the three main callbacks: orderbook update, trade, ticker.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FeedEvent {
    /// Full or incremental order book update (strategies: on_orderbook_update).
    OrderBookSnapshot(OrderBookSnapshot),
    OrderBookDelta(OrderBookDelta),
    /// Public trade (strategies: on_trade).
    Trade(TradeEvent),
    /// Ticker update e.g. last price, 24h volume (strategies: on_ticker_update).
    Ticker(TickerUpdate),
}

/// Ticker summary (last price, volume, best bid/ask).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TickerUpdate {
    pub symbol: String,
    pub last_price: hft_core::Price,
    pub volume_24h: Option<hft_core::Qty>,
    pub best_bid: Option<hft_core::Price>,
    pub best_ask: Option<hft_core::Price>,
}

/// Feed handler: subscribes to connector, updates order book, broadcasts FeedEvent.
/// When `fill_event_tx` is set (e.g. simulator), forwards [FillEvent] from connector so UI/strategy get real fills.
pub struct FeedHandler {
    connector: Arc<dyn hft_exchange::ExchangeConnector>,
    order_book: Arc<OrderBook>,
    tx: broadcast::Sender<EventEnvelope<FeedEvent>>,
    capacity: usize,
    fill_event_tx: Option<mpsc::UnboundedSender<FillEvent>>,
}

impl FeedHandler {
    /// Create a new feed handler. Returns the handler and the broadcast sender.
    /// Call `feed_tx.subscribe()` for each consumer (e.g. UI and strategy engine).
    /// If `fill_event_tx` is Some (simulator with real fills), connector fill messages are forwarded there.
    pub fn new(
        connector: Arc<dyn hft_exchange::ExchangeConnector>,
        order_book: Arc<OrderBook>,
        capacity: usize,
        fill_event_tx: Option<mpsc::UnboundedSender<FillEvent>>,
    ) -> (Self, broadcast::Sender<EventEnvelope<FeedEvent>>) {
        let (tx, _rx) = broadcast::channel(capacity);
        let handler = Self {
            connector,
            order_book,
            tx: tx.clone(),
            capacity,
            fill_event_tx,
        };
        (handler, tx)
    }

    /// Run the feed loop: subscribe to connector and for each message update book and broadcast.
    pub async fn run(self) -> hft_core::Result<()> {
        let mut rx = self.connector.subscribe().await?;
        let order_book = self.order_book.clone();
        let tx = self.tx;

        while let Some(msg) = rx.recv().await {
            let envelope = match msg {
                ExchangeMessage::OrderBookSnapshot(snap) => {
                    order_book.replace(snap.bids.clone(), snap.asks.clone());
                    EventEnvelope {
                        source: EventSource::FeedHandler,
                        ts: Utc::now(),
                        payload: FeedEvent::OrderBookSnapshot(snap),
                    }
                }
                ExchangeMessage::OrderBookDelta(delta) => {
                    let bid_tuples: Vec<_> = delta.bids.iter().map(|l| (l.price, l.qty)).collect();
                    let ask_tuples: Vec<_> = delta.asks.iter().map(|l| (l.price, l.qty)).collect();
                    if !bid_tuples.is_empty() {
                        order_book.update_bids(&bid_tuples);
                    }
                    if !ask_tuples.is_empty() {
                        order_book.update_asks(&ask_tuples);
                    }
                    EventEnvelope {
                        source: EventSource::FeedHandler,
                        ts: Utc::now(),
                        payload: FeedEvent::OrderBookDelta(delta),
                    }
                }
                ExchangeMessage::Trade(trade) => EventEnvelope {
                    source: EventSource::FeedHandler,
                    ts: Utc::now(),
                    payload: FeedEvent::Trade(trade),
                },
                ExchangeMessage::OrderEvent(_) => {
                    debug!(?msg, "feed handler ignoring order event");
                    continue;
                }
                ExchangeMessage::Fill(fill) => {
                    if let Some(ref fill_tx) = self.fill_event_tx {
                        let _ = fill_tx.send(fill);
                    }
                    continue;
                }
                ExchangeMessage::Connected
                | ExchangeMessage::Disconnected { .. }
                | ExchangeMessage::Debug(_) => {
                    debug!(?msg, "feed handler ignoring non-feed message");
                    continue;
                }
            };
            let _ = tx.send(envelope);
        }
        Ok(())
    }
}
