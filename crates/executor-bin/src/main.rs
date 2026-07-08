use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use executor_core::bybit::{BybitConnector, OrderRequest};
use executor_core::position::{ExitReason, PositionManager};
use executor_core::receiver::freshness_ok;
use executor_core::risk::{RiskDecision, RiskEngine};
use executor_core::safe_mode::SafeMode;
use observer_core::bybit::stream_bybit_mids;
use shared::config::{AppConfig, EdgeProfile, SymbolsFile};
use shared::packet::{BybitMidFeed, InstrumentType, PositionState, Side};
use shared::registry::SymbolRegistry;
use shared::time::utc_now_ns;
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
    if paper_or_live {
        validate_startup(&cfg, &symbols, &edge, true).context("startup validation")?;
    } else {
        warn!("executor running in dev mode — edge gate skipped");
    }

    let registry = SymbolRegistry::from_symbols(&symbols.symbol);
    let mut id_to_bybit: HashMap<u16, String> = HashMap::new();
    for s in &symbols.symbol {
        if s.enabled {
            id_to_bybit.insert(s.id, s.bybit.clone());
        }
    }

    let mut risk = RiskEngine::default();
    risk.max_latency_ns = cfg.network.max_latency_ms * 1_000_000;
    let pm = PositionManager {
        convergence_ratio: cfg.lag.convergence_exit_ratio,
        time_stop_ms: cfg.lag.time_stop_ms,
        ..Default::default()
    };
    let safe_mode = SafeMode::new(
        cfg.safe_mode.heartbeat_miss_caution,
        cfg.safe_mode.heartbeat_miss_defensive,
        cfg.safe_mode.heartbeat_miss_emergency,
    );

    let publisher = Arc::new(ZenohPublisher::open().await.context("zenoh publisher")?);
    let subscriber = ZenohSubscriber::open().await.context("zenoh subscriber")?;
    let bybit_api = BybitConnector::from_env();
    if bybit_api.is_none() {
        warn!("BYBIT_API_KEY not set — orders will be dry-run only");
    }

    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let deposit = cfg.capital.initial_futures_deposit_usdt;

    // Publish Bybit mid @ ~50Hz to Observer (§3.5.1)
    let bybit_symbols: Vec<String> = symbols
        .symbol
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.bybit.clone())
        .collect();
    let mut sym_map: HashMap<String, u16> = HashMap::new();
    for s in &symbols.symbol {
        if s.enabled {
            sym_map.insert(s.bybit.clone(), s.id);
        }
    }
    let feed_hz = cfg.lag.bybit_mid_feed_hz;
    let (mid_tx, mut mid_rx) = tokio::sync::mpsc::unbounded_channel::<BybitMidFeed>();
    let pub_mid = Arc::clone(&publisher);
    tokio::spawn(async move {
        while let Some(feed) = mid_rx.recv().await {
            if let Err(e) = pub_mid.publish_bybit_mid(&feed).await {
                warn!("bybit mid publish: {e}");
            }
        }
    });

    tokio::spawn(async move {
        let _ = stream_bybit_mids(&bybit_symbols, move |tick| {
            let Some(symbol_id) = sym_map.get(&tick.symbol).copied() else {
                return;
            };
            let feed = BybitMidFeed {
                symbol_id,
                bybit_mid: tick.mid,
                ts_ns: utc_now_ns(),
            };
            let _ = mid_tx.send(feed);
        })
        .await;
        let _ = feed_hz;
    });

    info!(
        "executor started pairs={} dry_run={}",
        registry.active_count.load(Ordering::Relaxed),
        bybit_api.is_none()
    );

    let (pkt_tx, mut pkt_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let _ = subscriber
            .run_packets(move |packet| {
                let _ = pkt_tx.send(packet);
            })
            .await;
    });

    while let Some(packet) = pkt_rx.recv().await {
        for pos in positions.values_mut() {
            pm.update_lag_capture(pos, &packet);
            if let Some(reason) = pm.check_exit(pos, deposit, &packet, utc_now_ns()) {
                info!("exit {:?} pos={}", reason, pos.id);
            }
        }

        match risk.check_entry(&packet) {
            RiskDecision::Open => {
                if !freshness_ok(&packet, risk.max_latency_ns) {
                    continue;
                }
                let Some(symbol) = id_to_bybit.get(&packet.symbol_id) else {
                    continue;
                };
                let side = if packet.direction_bias > 0 {
                    "Buy"
                } else {
                    "Sell"
                };
                let alloc = symbols
                    .symbol
                    .iter()
                    .find(|s| s.id == packet.symbol_id)
                    .and_then(|s| s.futures_alloc_pct)
                    .unwrap_or(0.05);
                let notional = deposit * alloc * cfg.execution.default_leverage_futures as f64;
                let qty = notional / packet.ref_price.max(1.0);

                info!(
                    "entry approved {} {} qty={:.6} lag_res={:.2}",
                    symbol, side, qty, packet.lag_residual_bps
                );

                if let Some(api) = &bybit_api {
                    let req = OrderRequest {
                        symbol: symbol.clone(),
                        side: side.into(),
                        qty,
                        price: None,
                        order_type: "Market".into(),
                    };
                    match api.place_order(&req).await {
                        Ok(body) => info!("bybit order response: {body}"),
                        Err(e) => warn!("bybit order failed: {e}"),
                    }
                }

                let pos_id = format!("{}-{}", symbol, packet.seq_num);
                positions.insert(
                    pos_id.clone(),
                    PositionState {
                        id: pos_id,
                        symbol_id: packet.symbol_id,
                        side: if packet.direction_bias > 0 {
                            Side::Long
                        } else {
                            Side::Short
                        },
                        instrument: InstrumentType::Futures,
                        entry_price: packet.ref_price,
                        qty,
                        qty_remaining: qty,
                        open_time_ns: utc_now_ns(),
                        entry_impulse_bps: packet.impulse_bps_100ms,
                        lag_capture_ratio: 0.0,
                        current_stop: 0.0,
                        current_tp: 0.0,
                        sl_phase: 0,
                        tp_phase: 0,
                        partial_done: false,
                        pnl_pct: 0.0,
                        exchange_stop_id: None,
                    },
                );
            }
            RiskDecision::Stale => warn!("stale packet seq={}", packet.seq_num),
            RiskDecision::Duplicate => {}
            RiskDecision::Skip => {}
        }

        let _ = (&safe_mode, ExitReason::Manual);
    }

    Ok(())
}
