use hmac::{Hmac, Mac};
use sha2::Sha256;
use shared::time::utc_now_ns;

type HmacSha256 = Hmac<Sha256>;

pub fn sign_bybit(secret: &str, payload: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: Option<f64>,
    pub order_type: String,
    /// Trigger price for StopMarket / TakeProfitMarket.
    pub trigger_price: Option<f64>,
    pub reduce_only: bool,
}

impl OrderRequest {
    pub fn market(symbol: impl Into<String>, side: impl Into<String>, qty: f64) -> Self {
        Self {
            symbol: symbol.into(),
            side: side.into(),
            qty,
            price: None,
            order_type: "Market".into(),
            trigger_price: None,
            reduce_only: false,
        }
    }

    pub fn market_reduce(symbol: impl Into<String>, side: impl Into<String>, qty: f64) -> Self {
        Self {
            reduce_only: true,
            ..Self::market(symbol, side, qty)
        }
    }

    pub fn stop_market(
        symbol: impl Into<String>,
        side: impl Into<String>,
        qty: f64,
        trigger: f64,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            side: side.into(),
            qty,
            price: None,
            order_type: "Market".into(),
            trigger_price: Some(trigger),
            reduce_only: true,
        }
    }
}

#[derive(Clone)]
pub struct BybitConnector {
    pub api_key: String,
    pub api_secret: String,
    pub testnet: bool,
    client: reqwest::Client,
}

impl BybitConnector {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("BYBIT_API_KEY").ok()?;
        let api_secret = std::env::var("BYBIT_API_SECRET").ok()?;
        let testnet = std::env::var("BYBIT_TESTNET")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        Some(Self::new(api_key, api_secret, testnet))
    }

    pub fn new(api_key: String, api_secret: String, testnet: bool) -> Self {
        Self {
            api_key,
            api_secret,
            testnet,
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> &'static str {
        if self.testnet {
            "https://api-testnet.bybit.com"
        } else {
            "https://api.bybit.com"
        }
    }

    pub fn build_order_payload(&self, req: &OrderRequest, ts_ms: u64) -> String {
        format!(
            r#"{{"category":"linear","symbol":"{}","side":"{}","orderType":"{}","qty":"{:.6}","price":"{}","timeInForce":"IOC","timestamp":{}}}"#,
            req.symbol,
            req.side,
            req.order_type,
            req.qty,
            req.price.map(|p| format!("{p:.2}")).unwrap_or_default(),
            ts_ms
        )
    }

    pub fn sign_order(&self, payload: &str) -> String {
        sign_bybit(&self.api_secret, payload)
    }

    pub async fn place_order(&self, req: &OrderRequest) -> anyhow::Result<String> {
        let ts = utc_now_ns() / 1_000_000;
        let recv_window = 5000u64;
        let mut body = serde_json::json!({
            "category": "linear",
            "symbol": req.symbol,
            "side": req.side,
            "orderType": req.order_type,
            "qty": format!("{:.6}", req.qty),
            "timeInForce": "GTC",
            "reduceOnly": req.reduce_only,
        });
        if let Some(trig) = req.trigger_price {
            body["orderType"] = serde_json::json!("Market");
            body["triggerPrice"] = serde_json::json!(format!("{trig:.2}"));
            body["triggerDirection"] = serde_json::json!(if req.side.eq_ignore_ascii_case("Sell") {
                2
            } else {
                1
            });
            body["triggerBy"] = serde_json::json!("LastPrice");
            body["timeInForce"] = serde_json::json!("GTC");
        } else {
            body["timeInForce"] = serde_json::json!("IOC");
        }
        let body_str = body.to_string();
        let sign_payload = format!("{ts}{}{recv_window}{body_str}", self.api_key);
        let sign = sign_bybit(&self.api_secret, &sign_payload);
        let url = format!("{}/v5/order/create", self.base_url());
        let resp = self
            .client
            .post(url)
            .header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", recv_window.to_string())
            .header("X-BAPI-SIGN", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?
            .error_for_status()?;
        let text = resp.text().await?;
        ensure_bybit_ok(&text)?;
        Ok(text)
    }

    pub async fn cancel_order(&self, symbol: &str, order_id: &str) -> anyhow::Result<String> {
        let ts = utc_now_ns() / 1_000_000;
        let recv_window = 5000u64;
        let body = serde_json::json!({
            "category": "linear",
            "symbol": symbol,
            "orderId": order_id,
        });
        let body_str = body.to_string();
        let sign_payload = format!("{ts}{}{recv_window}{body_str}", self.api_key);
        let sign = sign_bybit(&self.api_secret, &sign_payload);
        let url = format!("{}/v5/order/cancel", self.base_url());
        let resp = self
            .client
            .post(url)
            .header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", recv_window.to_string())
            .header("X-BAPI-SIGN", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?
            .error_for_status()?;
        let text = resp.text().await?;
        ensure_bybit_ok(&text)?;
        Ok(text)
    }

    pub async fn get_ticker_last(&self, symbol: &str) -> anyhow::Result<f64> {
        let url = format!(
            "{}/v5/market/tickers?category=linear&symbol={symbol}",
            self.base_url()
        );
        let resp = self.client.get(url).send().await?.error_for_status()?;
        let v: serde_json::Value = resp.json().await?;
        let last = v["result"]["list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|row| row["lastPrice"].as_str())
            .ok_or_else(|| anyhow::anyhow!("ticker missing lastPrice: {v}"))?;
        last.parse::<f64>()
            .map_err(|e| anyhow::anyhow!("parse lastPrice: {e}"))
    }

    pub async fn fetch_instrument(&self, symbol: &str) -> anyhow::Result<InstrumentFilters> {
        let url = format!(
            "{}/v5/market/instruments-info?category=linear&symbol={symbol}",
            self.base_url()
        );
        let resp = self.client.get(url).send().await?.error_for_status()?;
        let text = resp.text().await?;
        ensure_bybit_ok(&text)?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let row = v["result"]["list"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow::anyhow!("instruments-info empty for {symbol}"))?;
        let lot = &row["lotSizeFilter"];
        let price = &row["priceFilter"];
        Ok(InstrumentFilters {
            qty_step: parse_f64_field(lot, "qtyStep").unwrap_or(0.001),
            min_qty: parse_f64_field(lot, "minOrderQty").unwrap_or(0.001),
            tick_size: parse_f64_field(price, "tickSize").unwrap_or(0.1),
        })
    }

    pub async fn set_leverage(&self, symbol: &str, leverage: u32) -> anyhow::Result<()> {
        let ts = utc_now_ns() / 1_000_000;
        let recv_window = 5000u64;
        let body = serde_json::json!({
            "category": "linear",
            "symbol": symbol,
            "buyLeverage": leverage.to_string(),
            "sellLeverage": leverage.to_string(),
        });
        let body_str = body.to_string();
        let sign_payload = format!("{ts}{}{recv_window}{body_str}", self.api_key);
        let sign = sign_bybit(&self.api_secret, &sign_payload);
        let url = format!("{}/v5/position/set-leverage", self.base_url());
        let resp = self
            .client
            .post(url)
            .header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", recv_window.to_string())
            .header("X-BAPI-SIGN", sign)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?
            .error_for_status()?;
        let text = resp.text().await?;
        // Already set leverage often returns non-zero; treat as soft ok if message says not modified
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let code = v["retCode"].as_i64().unwrap_or(-1);
            if code == 0 || code == 110043 {
                return Ok(());
            }
        }
        ensure_bybit_ok(&text)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InstrumentFilters {
    pub qty_step: f64,
    pub min_qty: f64,
    pub tick_size: f64,
}

impl InstrumentFilters {
    pub fn round_qty(&self, qty: f64) -> Option<f64> {
        if qty <= 0.0 {
            return None;
        }
        let step = if self.qty_step > 0.0 {
            self.qty_step
        } else {
            0.001
        };
        let q = (qty / step).floor() * step;
        if q + 1e-12 < self.min_qty {
            None
        } else {
            Some(q)
        }
    }

    pub fn round_price(&self, price: f64) -> f64 {
        let tick = if self.tick_size > 0.0 {
            self.tick_size
        } else {
            0.1
        };
        (price / tick).round() * tick
    }
}

fn parse_f64_field(obj: &serde_json::Value, key: &str) -> Option<f64> {
    obj.get(key)
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse().ok())
}

pub fn ensure_bybit_ok(body: &str) -> anyhow::Result<()> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("bybit JSON parse: {e}; body={body}"))?;
    let code = v["retCode"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = v["retMsg"].as_str().unwrap_or("");
        anyhow::bail!("bybit retCode={code} retMsg={msg} body={body}");
    }
    Ok(())
}

pub fn extract_order_id(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["result"]["orderId"].as_str().map(|s| s.to_string())
}

/// Confirmed fill from Bybit (never invent mid as fill).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderFill {
    pub avg_price: f64,
    pub cum_qty: f64,
    pub fully_filled: bool,
}

pub fn parse_order_fill(body: &str) -> Option<OrderFill> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let r = &v["result"];
    // create response may only have orderId — caller should poll get_order
    let avg = r
        .get("avgPrice")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|p| *p > 0.0 && p.is_finite())?;
    let cum = r
        .get("cumExecQty")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            r.get("qty")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse().ok())
        })
        .filter(|q| *q > 0.0 && q.is_finite())?;
    let status = r
        .get("orderStatus")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let fully = status.eq_ignore_ascii_case("Filled")
        || (status.eq_ignore_ascii_case("PartiallyFilledCanceled") && cum > 0.0);
    Some(OrderFill {
        avg_price: avg,
        cum_qty: cum,
        fully_filled: fully || status.is_empty(),
    })
}

impl BybitConnector {
    async fn signed_get(&self, path_query: &str) -> anyhow::Result<String> {
        let ts = utc_now_ns() / 1_000_000;
        let recv_window = 5000u64;
        // path_query like "/v5/order/realtime?category=linear&symbol=BTCUSDT&orderId=..."
        let q = path_query.split_once('?').map(|(_, q)| q).unwrap_or("");
        let sign_payload = format!("{ts}{}{recv_window}{q}", self.api_key);
        let sign = sign_bybit(&self.api_secret, &sign_payload);
        let url = format!("{}{path_query}", self.base_url());
        let resp = self
            .client
            .get(url)
            .header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-TIMESTAMP", ts.to_string())
            .header("X-BAPI-RECV-WINDOW", recv_window.to_string())
            .header("X-BAPI-SIGN", sign)
            .send()
            .await?
            .error_for_status()?;
        let text = resp.text().await?;
        ensure_bybit_ok(&text)?;
        Ok(text)
    }

    /// Poll order until fill fields appear or attempts exhausted.
    pub async fn await_order_fill(
        &self,
        symbol: &str,
        order_id: &str,
        attempts: u32,
    ) -> anyhow::Result<OrderFill> {
        if let Some(f) = self.get_order_fill(symbol, order_id).await? {
            if f.cum_qty > 0.0 && f.avg_price > 0.0 {
                return Ok(f);
            }
        }
        for _ in 0..attempts.max(1) {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            if let Some(f) = self.get_order_fill(symbol, order_id).await? {
                if f.cum_qty > 0.0 && f.avg_price > 0.0 {
                    return Ok(f);
                }
            }
        }
        anyhow::bail!("no fill for order {order_id} on {symbol}")
    }

    pub async fn get_order_fill(
        &self,
        symbol: &str,
        order_id: &str,
    ) -> anyhow::Result<Option<OrderFill>> {
        let path = format!(
            "/v5/order/realtime?category=linear&symbol={symbol}&orderId={order_id}"
        );
        let text = self.signed_get(&path).await?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let row = v["result"]["list"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(v["result"].clone());
        let wrapped = serde_json::json!({ "result": row });
        Ok(parse_order_fill(&wrapped.to_string()))
    }

    /// Open linear positions (size > 0).
    pub async fn fetch_open_positions(&self) -> anyhow::Result<Vec<ExchangePosition>> {
        let path = "/v5/position/list?category=linear&settleCoin=USDT";
        let text = self.signed_get(path).await?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let mut out = Vec::new();
        if let Some(list) = v["result"]["list"].as_array() {
            for row in list {
                let size: f64 = row["size"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                if size <= 0.0 {
                    continue;
                }
                let symbol = row["symbol"].as_str().unwrap_or("").to_string();
                let side_s = row["side"].as_str().unwrap_or("");
                let avg = row["avgPrice"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                out.push(ExchangePosition {
                    symbol,
                    side_buy: side_s.eq_ignore_ascii_case("Buy"),
                    size,
                    avg_price: avg,
                });
            }
        }
        Ok(out)
    }

    /// Public funding rate from linear ticker (no auth). Returns NaN if missing.
    pub async fn fetch_funding_rate(&self, symbol: &str) -> anyhow::Result<f64> {
        let url = format!(
            "{}/v5/market/tickers?category=linear&symbol={symbol}",
            self.base_url()
        );
        let resp = self.client.get(url).send().await?.error_for_status()?;
        let v: serde_json::Value = resp.json().await?;
        let rate = v["result"]["list"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|row| row["fundingRate"].as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(f64::NAN);
        Ok(rate)
    }

    /// Max |fundingRate| across symbols (NaN skipped).
    pub async fn fetch_max_abs_funding(&self, symbols: &[String]) -> anyhow::Result<f64> {
        let mut max_abs = 0.0_f64;
        for s in symbols {
            match self.fetch_funding_rate(s).await {
                Ok(r) if r.is_finite() => {
                    max_abs = max_abs.max(r.abs());
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("funding {s}: {e}"),
            }
        }
        Ok(max_abs)
    }
}

#[derive(Debug, Clone)]
pub struct ExchangePosition {
    pub symbol: String,
    pub side_buy: bool,
    pub size: f64,
    pub avg_price: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_market_sets_trigger() {
        let r = OrderRequest::stop_market("BTCUSDT", "Sell", 0.01, 65_000.0);
        assert!(r.reduce_only);
        assert_eq!(r.trigger_price, Some(65_000.0));
    }

    #[test]
    fn ensure_ok_accepts_zero() {
        assert!(ensure_bybit_ok(r#"{"retCode":0,"retMsg":"OK","result":{}}"#).is_ok());
        assert!(ensure_bybit_ok(r#"{"retCode":10001,"retMsg":"fail"}"#).is_err());
    }

    #[test]
    fn round_qty_respects_min() {
        let f = InstrumentFilters {
            qty_step: 0.001,
            min_qty: 0.001,
            tick_size: 0.1,
        };
        assert_eq!(f.round_qty(0.0015), Some(0.001));
        assert_eq!(f.round_qty(0.0004), None);
    }

    #[test]
    fn parse_fill_from_realtime() {
        let body = r#"{"result":{"orderId":"1","avgPrice":"65000.5","cumExecQty":"0.01","orderStatus":"Filled"}}"#;
        let f = parse_order_fill(body).unwrap();
        assert!((f.avg_price - 65000.5).abs() < 1e-9);
        assert!((f.cum_qty - 0.01).abs() < 1e-12);
        assert!(f.fully_filled);
    }
}
