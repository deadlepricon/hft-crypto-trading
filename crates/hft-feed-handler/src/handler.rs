//! Feed handler: consume exchange messages, update order book, broadcast events.
//!
//! Runs as a long-lived task. Receives [ExchangeMessage] from the connector,
//! applies order book updates to [OrderBook], and sends normalized events
//! (e.g. OrderBookSnapshot, TradeEvent) to downstream channels for the
//! strategy engine and UI.

use chrono::Utc;
use hft_core::events::{EventEnvelope, EventSource, OrderBookDelta, OrderBookSnapshot, TradeEvent};
use hft_exchange::{ExchangeConnector, ExchangeMessage};
use hft_order_book::OrderBook;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Feed handler: owns the order book and broadcasts market data events.
pub struct FeedHandler {
    connector: Arc<dyn ExchangeConnector>,
    order_book: Arc<OrderBook>,
    /// Channel to broadcast snapshots/deltas and trades to strategy and UI.
    tx_events: broadcast::Sender<EventEnvelope<FeedEvent>>,
}

/// Events emitted by the feed handler (order book or trade updates).
#[derive(Debug, Clone)]
pub enum FeedEvent {
    OrderBookSnapshot(OrderBookSnapshot),
    OrderBookDelta(OrderBookDelta),
    Trade(TradeEvent),
}

impl FeedHandler {
    /// Create a feed handler with the given connector and order book.
    /// Returns a broadcast sender for events (clone and give to strategy/UI).
    pub fn new(
        connector: Arc<dyn ExchangeConnector>,
        order_book: Arc<OrderBook>,
        event_capacity: usize,
    ) -> (Self, broadcast::Receiver<EventEnvelope<FeedEvent>>) {
        let (tx_events, rx_events) = broadcast::channel(event_capacity);
        let handler = Self {
            connector,
            order_book,
            tx_events,
        };
        (handler, rx_events)
    }

    /// Run the feed handler: subscribe to connector and process messages until disconnect.
    pub async fn run(&self) -> hft_core::Result<()> {
        let mut rx = self.connector.subscribe().await?;
        info!(exchange = %self.connector.name(), "feed handler subscribed");

        while let Some(msg) = rx.recv().await {
            match msg {
                ExchangeMessage::OrderBookSnapshot(snap) => {
                    self.order_book.replace(snap.bids.clone(), snap.asks.clone());
                    let envelope = EventEnvelope {
                        source: EventSource::FeedHandler,
                        ts: Utc::now(),
                        payload: FeedEvent::OrderBookSnapshot(snap),
                    };
                    let _ = self.tx_events.send(envelope);
                }
                ExchangeMessage::OrderBookDelta(delta) => {
                    let bids: Vec<_> = delta.bids.iter().map(|l| (l.price, l.qty)).collect();
                    let asks: Vec<_> = delta.asks.iter().map(|l| (l.price, l.qty)).collect();
                    if !bids.is_empty() {
                        self.order_book.update_bids(&bids);
                    }
                    if !asks.is_empty() {
                        self.order_book.update_asks(&asks);
                    }
                    let envelope = EventEnvelope {
                        source: EventSource::FeedHandler,
                        ts: Utc::now(),
                        payload: FeedEvent::OrderBookDelta(delta),
                    };
                    let _ = self.tx_events.send(envelope);
                }
                ExchangeMessage::Trade(trade) => {
                    let envelope = EventEnvelope {
                        source: EventSource::FeedHandler,
                        ts: Utc::now(),
                        payload: FeedEvent::Trade(trade),
                    };
                    let _ = self.tx_events.send(envelope);
                }
                ExchangeMessage::Connected => {
                    info!(exchange = %self.connector.name(), "connected");
                }
                ExchangeMessage::Disconnected { reason } => {
                    warn!(exchange = %self.connector.name(), %reason, "disconnected");
                }
                ExchangeMessage::OrderEvent(_) | ExchangeMessage::Fill(_) => {
                    // Execution path: can be forwarded to execution/UI; feed handler may ignore.
                }
            }
        }

        Ok(())
    }

    /// Clone of the event broadcast sender for wiring to strategy/UI.
    pub fn event_sender(&self) -> broadcast::Sender<EventEnvelope<FeedEvent>> {
        self.tx_events.clone()
    }

    /// Reference to the order book for direct snapshot access (e.g. by strategy or UI).
    pub fn order_book(&self) -> Arc<OrderBook> {
        Arc::clone(&self.order_book)
    }
}
