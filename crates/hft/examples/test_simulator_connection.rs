//! Quick test: connect to the simulator WebSocket and print a few messages.
//! Run: cargo run -p hft --example test_simulator_connection
//! Ensure the simulator is running at ws://localhost:8765/ws/feed

use hft_exchange::{ExchangeConnector, ExchangeMessage, SimulatorConnector, SIMULATOR_WS_URL};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,hft_exchange=debug")
        .init();

    let connector = SimulatorConnector::new("BTC/USDT");
    println!("Connecting to simulator at {}...", SIMULATOR_WS_URL);

    let mut rx = connector.subscribe().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut book_count = 0u32;
    let mut trade_count = 0u32;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(msg)) => {
                match &msg {
                    ExchangeMessage::Connected => println!("  [OK] Connected."),
                    ExchangeMessage::OrderBookDelta(d) => {
                        book_count += 1;
                        if book_count <= 3 {
                            println!(
                                "  [BOOK] {} bids, {} asks, seq {}",
                                d.bids.len(),
                                d.asks.len(),
                                d.sequence
                            );
                        }
                    }
                    ExchangeMessage::Trade(t) => {
                        trade_count += 1;
                        if trade_count <= 2 {
                            println!("  [TRADE] {:?} @ {} qty {}", t.side, t.price, t.qty);
                        }
                    }
                    ExchangeMessage::Disconnected { reason } => {
                        println!("  [DISCONNECTED] {}", reason);
                        break;
                    }
                    _ => {}
                }
                if book_count >= 5 && trade_count >= 2 {
                    println!("  Received enough messages; connection looks good.");
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }

    println!(
        "\nDone. Received {} book updates, {} trades.",
        book_count, trade_count
    );
    if book_count > 0 || trade_count > 0 {
        println!("Simulator connection OK.");
    } else {
        println!(
            "No market data received. Is the simulator running at {}?",
            SIMULATOR_WS_URL
        );
    }

    Ok(())
}
