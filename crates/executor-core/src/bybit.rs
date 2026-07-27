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
        Ok(resp.text().await?)
    }
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
}
