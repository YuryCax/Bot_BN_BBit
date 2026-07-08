use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use observer_core::entry::EntryEngine;
use observer_core::lag::LagState;
use observer_core::math::SymbolMetrics;
use shared::config::{AppConfig, EdgeProfile, SymbolsFile};
use shared::packet::MarketStatePacket;
use shared::packet_log::PacketLogWriter;
use shared::registry::SymbolRegistry;
use shared::validation::validate_startup;
use shared::zenoh_ipc::{ZenohPublisher, ZenohSubscriber};
use tracing::{info, warn};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let config_path =
        std::env::var("BOT_CONFIG").unwrap_or_else(|_| "config/config.toml".into());
    let symbols_path =
        std::env::var("BOT_SYMBOLS").unwrap_or_else(|_| "config/symbols.toml".into());

    let cfg = AppConfig::load(&config_path).context("load config")?;
    let symbols = SymbolsFile::load(&symbols_path).context("load symbols")?;
    let edge = EdgeProfile::load(&cfg.deployment.edge_profile_path).context("load edge")?;

    let paper_or_live = matches!(cfg.deployment.mode.as_str(), "paper" | "live" | "start");
    if let Err(e) = validate_startup(&cfg, &symbols, &edge, paper_or_live) {
        if paper_or_live {
            anyhow::bail!("startup validation failed: {e}");
        }
        warn!("dev mode validation warning: {e}");
    }

    let registry = SymbolRegistry::from_symbols(&symbols.symbol);
    let engine = EntryEngine::from_config(
        &cfg.lag,
        cfg.fees.d_min_net_futures(),
        cfg.signals.z_score_entry,
        cfg.signals.velocity_min,
    );

    let metrics: Arc<Mutex<HashMap<u16, SymbolMetrics>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let lags: Arc<Mutex<HashMap<u16, LagState>>> = Arc::new(Mutex::new(HashMap::new()));

    for s in &symbols.symbol {
        if s.enabled {
            metrics.lock().unwrap().insert(s.id, SymbolMetrics::default());
            lags.lock().unwrap().insert(
                s.id,
                LagState {
                    max_staleness_ms: cfg.lag.bybit_mid_max_staleness_ms,
                    ..Default::default()
                },
            );
        }
    }

    let publisher = Arc::new(ZenohPublisher::open().await.context("zenoh publisher")?);

    let log_path = std::env::var("BOT_PACKET_LOG").unwrap_or_else(|_| "logs/packets.bin".into());
    let packet_log = Arc::new(Mutex::new(
        PacketLogWriter::open(&log_path).context("open packet log")?,
    ));
    info!("packet log -> {log_path}");

    let seq = Arc::new(AtomicU32::new(1));
    let (pkt_tx, mut pkt_rx) = tokio::sync::mpsc::unbounded_channel::<MarketStatePacket>();

    let pub_task = Arc::clone(&publisher);
    let log_task = Arc::clone(&packet_log);
    tokio::spawn(async move {
        while let Some(pkt) = pkt_rx.recv().await {
            if let Err(e) = pub_task.publish_packet(&pkt).await {
                warn!("zenoh publish failed: {e}");
            }
            if let Err(e) = log_task.lock().unwrap().append(&pkt) {
                warn!("packet log write failed: {e}");
            }
        }
    });

    // Bybit mid reverse feed (Executor → Observer via Zenoh §3.5.1)
    let lags_feed = Arc::clone(&lags);
    tokio::spawn(async move {
        let sub = ZenohSubscriber::open().await;
        if let Ok(sub) = sub {
            let _ = sub
                .run_bybit_mid(move |feed| {
                    if let Some(state) = lags_feed.lock().unwrap().get_mut(&feed.symbol_id) {
                        state.apply_feed(&feed);
                    }
                })
                .await;
        }
    });

    info!(
        "observer started mode={} pairs={}",
        cfg.deployment.mode,
        registry.active_count.load(Ordering::Relaxed)
    );

    let ws_symbols: Vec<String> = symbols
        .symbol
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.binance.clone())
        .collect();

    let metrics_ws = Arc::clone(&metrics);
    let lags_ws = Arc::clone(&lags);
    let engine = Arc::new(engine);
    let edge = Arc::new(edge);
    let pkt_tx_ws = pkt_tx.clone();
    let seq_ws = Arc::clone(&seq);

    let mut sym_id_map: HashMap<String, u16> = HashMap::new();
    for s in &symbols.symbol {
        sym_id_map.insert(s.binance.clone(), s.id);
    }

    tokio::spawn(async move {
        let _ = observer_core::binance::stream_book_tickers(&ws_symbols, move |tick| {
            let Some(&sid) = sym_id_map.get(&tick.symbol) else {
                return;
            };
            let mut m = metrics_ws.lock().unwrap();
            let entry = m.entry(sid).or_default();
            entry.push_price(tick.mid);

            {
                let mut lag_map = lags_ws.lock().unwrap();
                if let Some(lag_state) = lag_map.get_mut(&sid) {
                    lag_state.binance_mid_100ms_ago = entry.price_100ms_ago(1);
                }
            }

            let lag = lags_ws.lock().unwrap();
            let lag_state = lag.get(&sid).cloned().unwrap_or_default();
            drop(lag);

            let hour = (shared::time::utc_now_ns() / 3_600_000_000_000) % 24;
            let trade_hour_ok = edge
                .edges
                .get(&tick.symbol)
                .map(|e| e.trade_hours_utc.is_empty() || e.trade_hours_utc.contains(&(hour as u32)))
                .unwrap_or(true);

            let residual = lag_state.lag_residual_bps(tick.mid);
            let impulse = lag_state.impulse_bps(tick.mid);
            let stale = lag_state.is_stale();

            let mut pkt = engine.evaluate(
                entry,
                tick.mid,
                residual,
                impulse,
                stale,
                trade_hour_ok,
            );
            pkt.symbol_id = sid;
            pkt.seq_num = seq_ws.fetch_add(1, Ordering::Relaxed);
            pkt.bybit_mid_ref = lag_state.bybit_mid;
            pkt.lag_bps = lag_state.lag_bps(tick.mid);
            pkt.lag_residual_bps = residual;
            pkt.impulse_bps_100ms = impulse;

            if pkt.entry_valid == 1 {
                info!(
                    "entry signal symbol={} dir={} lag_res={:.2}",
                    tick.symbol, pkt.direction_bias, pkt.lag_residual_bps
                );
            }

            let _ = pkt_tx_ws.send(pkt);
        })
        .await;
    });

    tokio::signal::ctrl_c().await?;
    info!("observer shutdown");
    Ok(())
}
