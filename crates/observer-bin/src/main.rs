//! ADR-003 thin forwarder: Binance WS → Zenoh BinanceTick + heartbeat.
//! Does NOT compute entry_valid.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use shared::config::{AppConfig, EdgeProfile, SymbolsFile};
use shared::packet::BinanceTick;
use shared::registry::SymbolRegistry;
use shared::time::utc_now_ns;
use shared::validation::validate_startup;
use shared::zenoh_ipc::ZenohPublisher;
use tracing::{info, warn};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config_path = std::env::var("BOT_CONFIG").unwrap_or_else(|_| "config/config.toml".into());
    let symbols_path =
        std::env::var("BOT_SYMBOLS").unwrap_or_else(|_| "config/symbols.toml".into());

    let cfg = AppConfig::load(&config_path).context("load config")?;
    let symbols = SymbolsFile::load(&symbols_path).context("load symbols")?;
    let edge = EdgeProfile::load(&cfg.deployment.edge_profile_path).context("load edge")?;

    let paper_or_live = matches!(
        cfg.deployment.mode.to_ascii_lowercase().as_str(),
        "paper" | "live"
    );
    let need_edge = match cfg.deployment.mode.to_ascii_lowercase().as_str() {
        "live" => true,
        "paper" => !cfg.deployment.allow_unverified_paper,
        _ => false,
    };
    validate_startup(&cfg, &symbols, &edge, need_edge).context("startup validation")?;
    if !need_edge {
        warn!(
            "observer mode={} — edge gate skipped (paper_or_live={paper_or_live})",
            cfg.deployment.mode
        );
    }

    let registry = SymbolRegistry::from_symbols(&symbols.symbol);
    let publisher = Arc::new(ZenohPublisher::open().await.context("zenoh publisher")?);

    let seq = Arc::new(AtomicU32::new(1));
    let mut sym_id_map: HashMap<String, u16> = HashMap::new();
    for s in &symbols.symbol {
        if s.enabled {
            sym_id_map.insert(s.binance.clone(), s.id);
        }
    }

    let ws_symbols: Vec<String> = symbols
        .symbol
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.binance.clone())
        .collect();

    info!(
        "observer forwarder started mode={} pairs={} (ADR-003: no entry engine)",
        cfg.deployment.mode,
        registry.active_count.load(Ordering::Relaxed)
    );

    let pub_hb = Arc::clone(&publisher);
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_millis(100));
        loop {
            iv.tick().await;
            if let Err(e) = pub_hb.publish_heartbeat(utc_now_ns()).await {
                warn!("heartbeat publish: {e}");
            }
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BinanceTick>();
    let pub_ticks = Arc::clone(&publisher);
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = pub_ticks.publish_binance_tick(&msg).await {
                warn!("tick publish: {e}");
            }
        }
    });

    let seq_ws = Arc::clone(&seq);
    tokio::spawn(async move {
        let _ = observer_core::binance::stream_book_tickers(&ws_symbols, move |tick| {
            let Some(&sid) = sym_id_map.get(&tick.symbol) else {
                return;
            };
            let msg = BinanceTick {
                symbol_id: sid,
                mid: tick.mid,
                ts_ns: utc_now_ns(),
                seq_num: seq_ws.fetch_add(1, Ordering::Relaxed),
            };
            let _ = tx.send(msg);
        })
        .await;
    });

    tokio::signal::ctrl_c().await?;
    info!("observer forwarder shutdown");
    Ok(())
}
