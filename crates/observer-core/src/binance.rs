use futures_util::StreamExt;
use simd_json::prelude::{ValueAsScalar, ValueObjectAccess};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone)]
pub struct BookTicker {
    pub symbol: String,
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
}

pub async fn stream_book_tickers(
    symbols: &[String],
    mut on_tick: impl FnMut(BookTicker) + Send + 'static,
) -> anyhow::Result<()> {
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@bookTicker", s.to_lowercase()))
        .collect();
    let url = format!(
        "wss://fstream.binance.com/stream?streams={}",
        streams.join("/")
    );

    loop {
        let (ws, _) = connect_async(&url).await?;
        let (_, mut read) = ws.split();
        while let Some(msg) = read.next().await {
            let msg = msg?;
            if let Message::Text(text) = msg {
                let mut buf = text;
                if let Ok(v) = unsafe { simd_json::from_str::<simd_json::OwnedValue>(&mut buf) } {
                    if let Some(data) = v.get("data") {
                        let sym = data.get("s").and_then(|x| x.as_str()).unwrap_or("");
                        let bid: f64 = data
                            .get("b")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let ask: f64 = data
                            .get("a")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        if bid > 0.0 && ask > 0.0 {
                            on_tick(BookTicker {
                                symbol: sym.to_string(),
                                bid,
                                ask,
                                mid: (bid + ask) / 2.0,
                            });
                        }
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
