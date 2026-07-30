//! Public Bybit market data (no auth) for warm-path risk.

use serde::Deserialize;

fn public_base(testnet: bool) -> &'static str {
    if testnet {
        "https://api-testnet.bybit.com"
    } else {
        "https://api.bybit.com"
    }
}

#[derive(Debug, Deserialize)]
struct TickerResp {
    result: TickerResult,
}

#[derive(Debug, Deserialize)]
struct TickerResult {
    list: Vec<TickerRow>,
}

#[derive(Debug, Deserialize)]
struct TickerRow {
    #[allow(dead_code)]
    symbol: String,
    #[serde(rename = "fundingRate")]
    funding_rate: Option<String>,
}

/// Max absolute funding rate across symbols (linear perps). Polls per-symbol.
pub async fn fetch_max_abs_funding_rate(
    client: &reqwest::Client,
    symbols: &[String],
    testnet: bool,
) -> anyhow::Result<f64> {
    if symbols.is_empty() {
        return Ok(0.0);
    }
    let mut max_abs = 0.0f64;
    for sym in symbols {
        let url = format!(
            "{}/v5/market/tickers?category=linear&symbol={sym}",
            public_base(testnet)
        );
        let resp: TickerResp = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        for row in resp.result.list {
            if let Some(s) = row.funding_rate {
                if let Ok(r) = s.parse::<f64>() {
                    max_abs = max_abs.max(r.abs());
                }
            }
        }
    }
    Ok(max_abs)
}

/// Convenience wrapper for background pollers (creates its own HTTP client).
pub async fn poll_max_abs_funding_rate(symbols: &[String], testnet: bool) -> anyhow::Result<f64> {
    let client = reqwest::Client::new();
    fetch_max_abs_funding_rate(&client, symbols, testnet).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_base_urls() {
        assert!(public_base(true).contains("testnet"));
        assert!(public_base(false).contains("bybit.com"));
    }
}
