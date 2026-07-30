//! Authenticated Bybit smoke: instrument → leverage → market open → fill → reduce-only close.
//! Requires BYBIT_API_KEY / BYBIT_API_SECRET. Defaults to testnet (BYBIT_TESTNET=1).
//! Refuse mainnet unless BOT_ALLOW_MAINNET_SMOKE=1.

use executor_core::bybit::{extract_order_id, BybitConnector, OrderRequest};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let api = BybitConnector::from_env().ok_or_else(|| {
        anyhow::anyhow!("set BYBIT_API_KEY and BYBIT_API_SECRET (see secrets.env.example)")
    })?;

    if !api.testnet {
        let allow = std::env::var("BOT_ALLOW_MAINNET_SMOKE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !allow {
            anyhow::bail!(
                "refusing mainnet order smoke — set BYBIT_TESTNET=1, or BOT_ALLOW_MAINNET_SMOKE=1 after staged gates"
            );
        }
        warn!("MAINNET smoke enabled via BOT_ALLOW_MAINNET_SMOKE");
    } else {
        info!("Bybit TESTNET smoke");
    }

    let symbol = std::env::var("SMOKE_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let lev: u32 = std::env::var("SMOKE_LEVERAGE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);

    let filt = api.fetch_instrument(&symbol).await?;
    info!(
        "instrument {symbol} qty_step={} min_qty={} tick={}",
        filt.qty_step, filt.min_qty, filt.tick_size
    );

    let last = api.get_ticker_last(&symbol).await?;
    let qty = filt
        .round_qty(filt.min_qty)
        .ok_or_else(|| anyhow::anyhow!("cannot round min qty"))?;
    info!("last={last:.2} smoke_qty={qty}");

    if let Err(e) = api.set_leverage(&symbol, lev).await {
        warn!("set_leverage: {e}");
    }

    // Open long market
    let open = OrderRequest::market(&symbol, "Buy", qty);
    let body = api.place_order(&open).await?;
    info!("open: {body}");
    let oid = extract_order_id(&body).ok_or_else(|| anyhow::anyhow!("no orderId on open"))?;
    let fill = api.await_order_fill(&symbol, &oid, 50).await?;
    info!(
        "open fill avg={:.4} qty={:.6} full={}",
        fill.avg_price, fill.cum_qty, fill.fully_filled
    );

    let positions = api.fetch_open_positions().await?;
    let ours = positions.iter().find(|p| p.symbol == symbol);
    info!(
        "positions after open: {:?}",
        ours.map(|p| (p.size, p.avg_price))
    );

    // Close reduce-only
    let close_qty = fill.cum_qty.max(qty);
    let close = OrderRequest::market_reduce(&symbol, "Sell", close_qty);
    let cbody = api.place_order(&close).await?;
    info!("close: {cbody}");
    if let Some(cid) = extract_order_id(&cbody) {
        match api.await_order_fill(&symbol, &cid, 50).await {
            Ok(cf) => info!("close fill avg={:.4} qty={:.6}", cf.avg_price, cf.cum_qty),
            Err(e) => warn!("close fill poll: {e}"),
        }
    }

    let after = api.fetch_open_positions().await?;
    let left = after
        .iter()
        .find(|p| p.symbol == symbol)
        .map(|p| p.size)
        .unwrap_or(0.0);
    if left > filt.min_qty * 0.5 {
        anyhow::bail!("position still open size={left} — flatten manually on testnet");
    }

    info!("SMOKE OK: open+fill+reduce-only close on {symbol}");
    Ok(())
}
