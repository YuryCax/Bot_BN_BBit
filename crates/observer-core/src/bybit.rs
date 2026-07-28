use futures_util::{SinkExt, StreamExt};
use simd_json::prelude::{ValueAsContainer, ValueAsScalar, ValueObjectAccess};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const BYBIT_WS_MAINNET: &str = "wss://stream.bybit.com/v5/public/linear";
const BYBIT_WS_TESTNET: &str = "wss://stream-testnet.bybit.com/v5/public/linear";

fn bybit_ws_url() -> String {
    if let Ok(url) = std::env::var("BYBIT_WS_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    let testnet = std::env::var("BYBIT_TESTNET")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if testnet {
        BYBIT_WS_TESTNET.to_string()
    } else {
        BYBIT_WS_MAINNET.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct BybitTick {
    pub symbol: String,
    pub mid: f64,
    /// Approximate top-of-book notional (bid×qty + ask×qty) in USD.
    pub top_depth_usd: f64,
}

pub async fn stream_bybit_mids(
    symbols: &[String],
    mut on_tick: impl FnMut(BybitTick) + Send + 'static,
) -> anyhow::Result<()> {
    let args: Vec<String> = symbols
        .iter()
        .map(|s| format!("orderbook.1.{s}"))
        .collect();
    let sub = serde_json::json!({ "op": "subscribe", "args": args });
    let sub_text = sub.to_string();
    let ws_url = bybit_ws_url();

    loop {
        let (ws, _) = connect_async(&ws_url).await?;
        let (mut write, mut read) = ws.split();
        write.send(Message::Text(sub_text.clone())).await?;

        while let Some(msg) = read.next().await {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let mut buf = text;
                let Ok(v) = (unsafe { simd_json::from_str::<simd_json::OwnedValue>(&mut buf) }) else {
                    continue;
                };
                let topic = v.get("topic").and_then(|x| x.as_str()).unwrap_or("");
                if !topic.starts_with("orderbook.1.") {
                    continue;
                }
                let sym = topic.rsplit('.').next().unwrap_or("");
                let Some(data) = v.get("data") else {
                    continue;
                };
                let bid: Option<f64> = data
                    .get("b")
                    .and_then(|b| b.as_array())
                    .and_then(|a| a.first())
                    .and_then(|row| row.as_array())
                    .and_then(|r| r.first())
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());
                let bid_qty: Option<f64> = data
                    .get("b")
                    .and_then(|b| b.as_array())
                    .and_then(|a| a.first())
                    .and_then(|row| row.as_array())
                    .and_then(|r| r.get(1))
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());
                let ask: Option<f64> = data
                    .get("a")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|row| row.as_array())
                    .and_then(|r| r.first())
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());
                let ask_qty: Option<f64> = data
                    .get("a")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.first())
                    .and_then(|row| row.as_array())
                    .and_then(|r| r.get(1))
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse().ok());
                if let (Some(bid), Some(ask)) = (bid, ask) {
                    if bid > 0.0 && ask > 0.0 {
                        let depth = bid * bid_qty.unwrap_or(0.0) + ask * ask_qty.unwrap_or(0.0);
                        on_tick(BybitTick {
                            symbol: sym.to_string(),
                            mid: (bid + ask) / 2.0,
                            top_depth_usd: depth,
                        });
                    }
                }
            }
        }
    }
}
