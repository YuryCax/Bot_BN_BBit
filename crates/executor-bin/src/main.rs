//! ADR-003 Executor: local Bybit mid + EntryEngine + Risk + orders + paper ledger.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use executor_core::bybit::{
    extract_order_id, BybitConnector, ExchangePosition, OrderRequest,
};
use executor_core::paper_ledger::PaperLedger;
use executor_core::position::PositionManager;
use executor_core::risk::{RiskDecision, RiskEngine};
use executor_core::safe_mode::{SafeMode, SafeModePhase};
use executor_core::trading_mode::{allow_live_orders, requires_edge_gate};
use executor_core::warm_risk::WarmRiskState;
use observer_core::bybit::stream_bybit_mids;
use observer_core::entry::EntryEngine;
use observer_core::follow_through::FollowThroughTracker;
use observer_core::lag::LagState;
use observer_core::math::SymbolMetrics;
use shared::config::{AppConfig, EdgeProfile, SymbolsFile};
use shared::packet::{
    BinanceTick, InstrumentType, OperatorAction, OperatorCommand, PositionState, Side,
};
use shared::packet_log::PacketLogWriter;
use shared::registry::SymbolRegistry;
use shared::time::utc_now_ns;
use shared::validation::validate_startup;
use shared::zenoh_ipc::ZenohSubscriber;
use tracing::{info, warn};

fn mid_for_symbol(
    lags: &Mutex<HashMap<u16, LagState>>,
    symbol_id: u16,
    fallback: f64,
) -> f64 {
    lags.lock()
        .unwrap()
        .get(&symbol_id)
        .map(|s| s.bybit_mid)
        .filter(|m| *m > 0.0)
        .unwrap_or(fallback)
}

async fn place_reduce_only_close(
    api: &BybitConnector,
    symbol: &str,
    close_side: &str,
    qty: f64,
) -> anyhow::Result<()> {
    let req = OrderRequest::market_reduce(symbol, close_side, qty);
    let body = api.place_order(&req).await?;
    if let Some(oid) = extract_order_id(&body) {
        match api.await_order_fill(symbol, &oid, 25).await {
            Ok(fill) => info!(
                "close fill {symbol} qty={:.6} px={:.4}",
                fill.cum_qty, fill.avg_price
            ),
            Err(e) => warn!("close fill poll {symbol}: {e}"),
        }
    }
    Ok(())
}

fn has_open_for_symbol(positions: &HashMap<String, PositionState>, symbol_id: u16) -> bool {
    positions.values().any(|p| p.symbol_id == symbol_id)
}

fn seed_positions_from_exchange(
    open: &[ExchangePosition],
    bybit_to_id: &HashMap<String, u16>,
    atr_mult: f64,
    tp_pct: f64,
    pm: &PositionManager,
) -> HashMap<String, PositionState> {
    let mut positions = HashMap::new();
    for ep in open {
        let Some(&sid) = bybit_to_id.get(&ep.symbol) else {
            warn!("reconcile: unknown exchange symbol {}", ep.symbol);
            continue;
        };
        let pos_side = if ep.side_buy { Side::Long } else { Side::Short };
        let (stop, tp) = pm.initial_stops(pos_side, ep.avg_price, 0.0, atr_mult, tp_pct);
        let pos_id = format!("reconcile-{}", ep.symbol);
        positions.insert(
            pos_id.clone(),
            PositionState {
                id: pos_id,
                symbol_id: sid,
                side: pos_side,
                instrument: InstrumentType::Futures,
                entry_price: ep.avg_price,
                qty: ep.size,
                qty_remaining: ep.size,
                open_time_ns: utc_now_ns(),
                entry_impulse_bps: 0.0,
                lag_capture_ratio: 0.0,
                current_stop: stop,
                current_tp: tp,
                sl_phase: 0,
                tp_phase: 0,
                partial_done: false,
                pnl_pct: 0.0,
                exchange_stop_id: None,
            },
        );
        info!(
            "reconcile seeded {} {:?} size={:.6} avg={:.4}",
            ep.symbol, pos_side, ep.size, ep.avg_price
        );
    }
    positions
}

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

    let need_edge = requires_edge_gate(
        &cfg.deployment.mode,
        cfg.deployment.allow_unverified_paper,
    );
    validate_startup(&cfg, &symbols, &edge, need_edge).context("startup validation")?;
    if !need_edge {
        warn!(
            "executor mode={} — edge gate skipped (allow_unverified_paper={})",
            cfg.deployment.mode, cfg.deployment.allow_unverified_paper
        );
    }

    let registry = SymbolRegistry::from_symbols(&symbols.symbol);
    let mut id_to_bybit: HashMap<u16, String> = HashMap::new();
    let mut id_to_binance: HashMap<u16, String> = HashMap::new();
    let mut bybit_to_id: HashMap<String, u16> = HashMap::new();
    let mut adverse_by_id: HashMap<u16, f32> = HashMap::new();
    for s in &symbols.symbol {
        if s.enabled {
            id_to_bybit.insert(s.id, s.bybit.clone());
            id_to_binance.insert(s.id, s.binance.clone());
            bybit_to_id.insert(s.bybit.clone(), s.id);
            if let Some(e) = edge.edges.get(&s.binance) {
                if let Some(bps) = e.max_adverse_move_bps {
                    adverse_by_id.insert(s.id, bps as f32);
                }
            }
        }
    }

    let engine = Arc::new(EntryEngine::from_config(
        &cfg.lag,
        cfg.fees.d_min_net_futures(),
        cfg.signals.z_score_entry,
        cfg.signals.velocity_min,
        cfg.risk.atr_min_filter,
    ));

    let metrics: Arc<Mutex<HashMap<u16, SymbolMetrics>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let lags: Arc<Mutex<HashMap<u16, LagState>>> = Arc::new(Mutex::new(HashMap::new()));
    let follow_through: Arc<Mutex<HashMap<u16, FollowThroughTracker>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let book_depths: Arc<Mutex<HashMap<u16, f64>>> = Arc::new(Mutex::new(HashMap::new()));

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
            let ft_min = edge
                .edges
                .get(&s.binance)
                .map(|e| e.follow_through_min as f32)
                .unwrap_or(cfg.lag.follow_through_min);
            follow_through
                .lock()
                .unwrap()
                .insert(s.id, FollowThroughTracker::new(ft_min, 200));
        }
    }

    let mut risk = RiskEngine::default();
    risk.max_latency_ns = cfg.network.max_latency_ms * 1_000_000;
    risk.max_adverse_move_bps = cfg.execution.max_adverse_move_bps as f32;

    let pm = PositionManager {
        convergence_ratio: cfg.lag.convergence_exit_ratio,
        time_stop_ms: cfg.lag.time_stop_ms,
        ..Default::default()
    };
    let mut safe_mode = SafeMode::new(
        cfg.safe_mode.heartbeat_miss_caution,
        cfg.safe_mode.heartbeat_miss_defensive,
        cfg.safe_mode.heartbeat_miss_emergency,
    );

    let warm = Arc::new(Mutex::new(WarmRiskState {
        max_daily_dd_futures: cfg.risk.max_daily_drawdown_futures,
        max_funding_rate: cfg.funding_basis.max_funding_rate,
        min_book_depth_usd: 5_000.0,
        book_depth_usd: 50_000.0,
        ..Default::default()
    }));
    warm.lock().unwrap().publish();

    let bybit_api = BybitConnector::from_env();
    let live_orders = allow_live_orders(&cfg.deployment.mode);
    if live_orders && bybit_api.is_none() {
        anyhow::bail!("mode=live requires BYBIT_API_KEY / BYBIT_API_SECRET");
    }
    if !live_orders {
        info!(
            "order routing: SIMULATION only (mode={}) — Bybit API will not receive orders",
            cfg.deployment.mode
        );
    } else {
        warn!("order routing: LIVE Bybit orders enabled");
    }
    let sim_ledger = !live_orders;

    let mut ledger = PaperLedger::default();
    let ledger_path =
        std::env::var("BOT_LEDGER").unwrap_or_else(|_| "logs/paper_ledger.jsonl".into());
    let log_path = std::env::var("BOT_PACKET_LOG").unwrap_or_else(|_| "logs/packets.bin".into());
    let packet_log = Arc::new(Mutex::new(
        PacketLogWriter::open(&log_path).context("open packet log")?,
    ));

    let mut positions: HashMap<String, PositionState> = HashMap::new();
    if live_orders {
        if let Some(api) = &bybit_api {
            match api.fetch_open_positions().await {
                Ok(open) => {
                    positions = seed_positions_from_exchange(
                        &open,
                        &bybit_to_id,
                        cfg.risk.atr_multiplier_stop,
                        cfg.take_profit.initial_target_pct,
                        &pm,
                    );
                }
                Err(e) => warn!("startup reconcile failed: {e}"),
            }
        }
    }

    let deposit = cfg.capital.initial_futures_deposit_usdt;
    let fee_rate = cfg.fees.futures_taker_pct;

    // Local Bybit mids
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
    let lags_by = Arc::clone(&lags);
    let warm_by = Arc::clone(&warm);
    let depths_by = Arc::clone(&book_depths);
    let funding_symbols = bybit_symbols.clone();
    tokio::spawn(async move {
        let _ = stream_bybit_mids(&bybit_symbols, move |tick| {
            let Some(symbol_id) = sym_map.get(&tick.symbol).copied() else {
                return;
            };
            if let Some(state) = lags_by.lock().unwrap().get_mut(&symbol_id) {
                state.bybit_mid = tick.mid;
                state.bybit_ts_ns = utc_now_ns();
            }
            if tick.top_depth_usd > 0.0 {
                depths_by
                    .lock()
                    .unwrap()
                    .insert(symbol_id, tick.top_depth_usd);
                let min_depth = depths_by
                    .lock()
                    .unwrap()
                    .values()
                    .copied()
                    .fold(f64::INFINITY, f64::min);
                if min_depth.is_finite() {
                    let mut w = warm_by.lock().unwrap();
                    w.book_depth_usd = min_depth;
                    w.publish();
                }
            }
        })
        .await;
    });

    let warm_funding = Arc::clone(&warm);
    let funding_testnet = bybit_api
        .as_ref()
        .map(|a| a.testnet)
        .unwrap_or(true);
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_secs(60));
        loop {
            iv.tick().await;
            match executor_core::market::poll_max_abs_funding_rate(&funding_symbols, funding_testnet)
                .await
            {
                Ok(rate) => {
                    let mut w = warm_funding.lock().unwrap();
                    w.funding_rate = rate;
                    w.publish();
                }
                Err(e) => warn!("funding poll: {e}"),
            }
        }
    });

    let subscriber = ZenohSubscriber::open().await.context("zenoh subscriber")?;
    let (tick_tx, mut tick_rx) = tokio::sync::mpsc::unbounded_channel::<BinanceTick>();
    let (hb_tx, mut hb_rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<OperatorCommand>();

    tokio::spawn(async move {
        let _ = subscriber
            .run_binance_ticks(move |tick| {
                let _ = tick_tx.send(tick);
            })
            .await;
    });
    let sub_hb = ZenohSubscriber::open().await.context("zenoh hb")?;
    tokio::spawn(async move {
        let _ = sub_hb
            .run_heartbeat(move |ts| {
                let _ = hb_tx.send(ts);
            })
            .await;
    });
    let sub_cmd = ZenohSubscriber::open().await.context("zenoh cmd")?;
    tokio::spawn(async move {
        let _ = sub_cmd
            .run_commands(move |cmd| {
                let _ = cmd_tx.send(cmd);
            })
            .await;
    });

    info!(
        "executor started ADR-003 mode={} pairs={} live_orders={} adverse_bps={:.1}",
        cfg.deployment.mode,
        registry.active_count.load(Ordering::Relaxed),
        live_orders,
        risk.max_adverse_move_bps
    );

    let heartbeat_timeout =
        Duration::from_millis(cfg.network.heartbeat_timeout_ms.max(500));
    let mut last_hb = Instant::now();
    let mut last_hb_ts_ns: u64 = 0;
    let pending_ft: Arc<Mutex<Vec<(u16, u64, i8, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let edge = Arc::new(edge);
    let mut operator_halt = false;
    let mut flatten_request = false;

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                let src = cmd.source.to_ascii_lowercase();
                if !(src == "panel" || src == "telegram" || src == "cli") {
                    warn!("ignoring operator cmd from untrusted source={}", cmd.source);
                    continue;
                }
                info!("operator cmd {:?} from {}", cmd.action, cmd.source);
                match cmd.action {
                    OperatorAction::HaltEntries => {
                        operator_halt = true;
                        {
                            let mut w = warm.lock().unwrap();
                            w.entries_halted = true;
                            w.publish();
                        }
                    }
                    OperatorAction::ResumeEntries => {
                        operator_halt = false;
                        {
                            let mut w = warm.lock().unwrap();
                            w.entries_halted = safe_mode.halt_entries();
                            w.publish();
                        }
                    }
                    OperatorAction::FlattenAll => {
                        flatten_request = true;
                        operator_halt = true;
                        {
                            let mut w = warm.lock().unwrap();
                            w.entries_halted = true;
                            w.publish();
                        }
                    }
                    OperatorAction::StatusPing => {
                        info!(
                            "status ping live_orders={} halt={} positions={} ledger_net={:.4} last_hb_age_ms={}",
                            live_orders,
                            operator_halt,
                            positions.len(),
                            ledger.net_pnl(),
                            last_hb.elapsed().as_millis()
                        );
                    }
                }
            }
            Some(ts) = hb_rx.recv() => {
                last_hb_ts_ns = ts;
                last_hb = Instant::now();
                safe_mode.on_heartbeat();
                {
                    let mut w = warm.lock().unwrap();
                    w.entries_halted = operator_halt || safe_mode.halt_entries();
                    w.day_pnl_pct = if deposit > 0.0 {
                        ledger.net_pnl() / deposit
                    } else {
                        0.0
                    };
                    w.publish();
                }
            }
            Some(tick) = tick_rx.recv() => {
                let sid = tick.symbol_id;
                let binance_sym = id_to_binance.get(&sid).cloned().unwrap_or_default();

                {
                    let mut m = metrics.lock().unwrap();
                    let entry = m.entry(sid).or_default();
                    entry.push_price(tick.mid);
                    let mut lag_map = lags.lock().unwrap();
                    if let Some(lag_state) = lag_map.get_mut(&sid) {
                        lag_state.binance_mid_100ms_ago = entry.price_at_age_ms(100);
                    }
                }

                let lag_state = lags
                    .lock()
                    .unwrap()
                    .get(&sid)
                    .cloned()
                    .unwrap_or_default();
                let metrics_snap = metrics
                    .lock()
                    .unwrap()
                    .get(&sid)
                    .cloned()
                    .unwrap_or_default();

                let now_ns = utc_now_ns();
                let hour = (now_ns / 3_600_000_000_000) % 24;
                let trade_hour_ok = edge
                    .edges
                    .get(&binance_sym)
                    .map(|e| e.trade_hours_utc.is_empty() || e.trade_hours_utc.contains(&(hour as u32)))
                    .unwrap_or(true);

                {
                    let mut pending = pending_ft.lock().unwrap();
                    let mut ft_map = follow_through.lock().unwrap();
                    pending.retain(|(psid, ts, dir, by_mid)| {
                        if now_ns.saturating_sub(*ts) < 300_000_000 {
                            return true;
                        }
                        if let Some(tracker) = ft_map.get_mut(psid) {
                            let by_now = lags
                                .lock()
                                .unwrap()
                                .get(psid)
                                .map(|s| s.bybit_mid)
                                .unwrap_or(*by_mid);
                            if *by_mid > 0.0 && by_now > 0.0 {
                                let ret = (by_now - *by_mid) / *by_mid;
                                tracker.record(ret * (*dir as f64) > 0.0);
                            }
                        }
                        false
                    });
                }

                let ft_ok = follow_through
                    .lock()
                    .unwrap()
                    .get(&sid)
                    .map(|t| t.allows_entry())
                    .unwrap_or(true);

                let residual = lag_state.lag_residual_bps(tick.mid);
                let impulse = lag_state.impulse_bps(tick.mid);
                let stale = lag_state.is_stale();

                let mut pkt = engine.evaluate(
                    &metrics_snap,
                    tick.mid,
                    residual,
                    impulse,
                    stale,
                    trade_hour_ok && ft_ok,
                );
                pkt.symbol_id = sid;
                pkt.seq_num = tick.seq_num;
                pkt.ts_ns = tick.ts_ns;
                pkt.bybit_mid_ref = lag_state.bybit_mid;
                pkt.lag_bps = lag_state.lag_bps(tick.mid);
                pkt.lag_residual_bps = residual;
                pkt.impulse_bps_100ms = impulse;
                pkt.ref_price = tick.mid;

                if let Err(e) = packet_log.lock().unwrap().append(&pkt) {
                    warn!("packet log: {e}");
                }

                // Exits
                let mut closed: Vec<String> = Vec::new();
                for (id, pos) in positions.iter_mut() {
                    pm.update_lag_capture(pos, &pkt);
                    let bybit_mid = if pkt.symbol_id == pos.symbol_id && pkt.bybit_mid_ref > 0.0 {
                        pkt.bybit_mid_ref
                    } else {
                        mid_for_symbol(&lags, pos.symbol_id, pos.entry_price)
                    };
                    if let Some(reason) = pm.check_exit(pos, bybit_mid, &pkt, utc_now_ns()) {
                        info!("exit {:?} pos={}", reason, pos.id);
                        if let Some(symbol) = id_to_bybit.get(&pos.symbol_id) {
                            let close_side = match pos.side {
                                Side::Long => "Sell",
                                Side::Short => "Buy",
                            };
                            if live_orders {
                                if let Some(api) = &bybit_api {
                                    if let Err(e) = place_reduce_only_close(
                                        api,
                                        symbol,
                                        close_side,
                                        pos.qty_remaining,
                                    )
                                    .await
                                    {
                                        warn!("exit order failed: {e}");
                                        continue;
                                    }
                                }
                            }
                            ledger.record_exit(
                                symbol,
                                close_side,
                                pos.qty_remaining,
                                pos.entry_price,
                                bybit_mid,
                                fee_rate,
                                sim_ledger,
                                &format!("{reason:?}"),
                            );
                            let _ = ledger.append_jsonl(&ledger_path);
                        }
                        closed.push(id.clone());
                    }
                }
                for id in closed {
                    positions.remove(&id);
                }

                if operator_halt || safe_mode.halt_entries() || warm.lock().unwrap().entries_halted {
                    // still allow flatten via timer branch
                } else if let Some(bps) = adverse_by_id.get(&sid) {
                    risk.max_adverse_move_bps = *bps;
                } else {
                    risk.max_adverse_move_bps = cfg.execution.max_adverse_move_bps as f32;
                }

                if !(operator_halt || safe_mode.halt_entries() || warm.lock().unwrap().entries_halted) {
                match risk.check_entry(&pkt) {
                    RiskDecision::Open => {
                        // Small-account monetization: at most one open futures position.
                        if !positions.is_empty() {
                            continue;
                        }
                        if has_open_for_symbol(&positions, sid) {
                            continue;
                        }
                        let Some(symbol) = id_to_bybit.get(&sid) else { continue };
                        let side = if pkt.direction_bias > 0 { "Buy" } else { "Sell" };
                        let alloc = symbols
                            .symbol
                            .iter()
                            .find(|s| s.id == sid)
                            .and_then(|s| s.futures_alloc_pct)
                            .unwrap_or(0.05);
                        let lev = symbols
                            .symbol
                            .iter()
                            .find(|s| s.id == sid)
                            .and_then(|s| s.leverage)
                            .unwrap_or(cfg.execution.default_leverage_futures);
                        let risk_frac = if cfg.deployment.mode.eq_ignore_ascii_case("live") {
                            cfg.capital.risk_per_trade_pct
                        } else {
                            alloc
                        };
                        let notional = deposit * risk_frac * lev as f64;
                        let mut qty = notional / pkt.ref_price.max(1.0);

                        let pos_side = if pkt.direction_bias > 0 {
                            Side::Long
                        } else {
                            Side::Short
                        };
                        let (mut stop, mut tp) = pm.initial_stops(
                            pos_side,
                            pkt.ref_price,
                            pkt.atr,
                            cfg.risk.atr_multiplier_stop,
                            cfg.take_profit.initial_target_pct,
                        );

                        let mut fill_px = pkt.ref_price;
                        let mut fill_qty = qty;
                        let mut stop_ok = true;

                        if live_orders {
                            if let Some(api) = &bybit_api {
                                match api.fetch_instrument(symbol).await {
                                    Ok(filt) => {
                                        stop = filt.round_price(stop);
                                        tp = filt.round_price(tp);
                                        match filt.round_qty(qty) {
                                            Some(q) => qty = q,
                                            None => {
                                                warn!("qty below min for {symbol}");
                                                continue;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("instrument {symbol}: {e}");
                                        continue;
                                    }
                                }
                                if let Err(e) = api.set_leverage(symbol, lev).await {
                                    warn!("set_leverage {symbol}: {e}");
                                }
                                let req = OrderRequest::market(symbol.clone(), side, qty);
                                match api.place_order(&req).await {
                                    Ok(body) => {
                                        info!("bybit order: {body}");
                                        if let Some(oid) = extract_order_id(&body) {
                                            match api.await_order_fill(symbol, &oid, 40).await {
                                                Ok(fill) => {
                                                    fill_px = fill.avg_price;
                                                    fill_qty = fill.cum_qty;
                                                }
                                                Err(e) => {
                                                    warn!("entry fill missing: {e}");
                                                    continue;
                                                }
                                            }
                                        } else {
                                            warn!("entry missing orderId");
                                            continue;
                                        }
                                    }
                                    Err(e) => {
                                        warn!("bybit order failed: {e}");
                                        continue;
                                    }
                                }
                                let stop_side = if pos_side == Side::Long { "Sell" } else { "Buy" };
                                let stop_req = OrderRequest::stop_market(
                                    symbol.clone(),
                                    stop_side,
                                    fill_qty,
                                    stop,
                                );
                                match api.place_order(&stop_req).await {
                                    Ok(body) => info!("bybit stop: {body}"),
                                    Err(e) => {
                                        warn!("bybit stop failed: {e} — halting entries");
                                        stop_ok = false;
                                        operator_halt = true;
                                        warm.lock().unwrap().entries_halted = true;
                                        warm.lock().unwrap().publish();
                                    }
                                }
                            }
                        }

                        info!(
                            "entry {} {} qty={:.6} fill_px={:.4} lag_res={:.2} live={}",
                            symbol, side, fill_qty, fill_px, pkt.lag_residual_bps, live_orders
                        );

                        ledger.record_entry(symbol, side, fill_qty, fill_px, fee_rate, sim_ledger);
                        let _ = ledger.append_jsonl(&ledger_path);

                        pending_ft.lock().unwrap().push((
                            sid,
                            now_ns,
                            pkt.direction_bias,
                            lag_state.bybit_mid,
                        ));

                        let pos_id = format!("{}-{}", symbol, pkt.seq_num);
                        positions.insert(
                            pos_id.clone(),
                            PositionState {
                                id: pos_id,
                                symbol_id: sid,
                                side: pos_side,
                                instrument: InstrumentType::Futures,
                                entry_price: fill_px,
                                qty: fill_qty,
                                qty_remaining: fill_qty,
                                open_time_ns: utc_now_ns(),
                                entry_impulse_bps: pkt.impulse_bps_100ms,
                                lag_capture_ratio: 0.0,
                                current_stop: stop,
                                current_tp: tp,
                                sl_phase: 0,
                                tp_phase: 0,
                                partial_done: false,
                                pnl_pct: 0.0,
                                exchange_stop_id: None,
                            },
                        );
                        let _ = stop_ok;
                    }
                    RiskDecision::Stale => warn!("stale tick seq={}", pkt.seq_num),
                    RiskDecision::AdverseMoveExceeded => {
                        warn!(
                            "adverse kill seq={} ref={:.2} bybit={:.2}",
                            pkt.seq_num, pkt.ref_price, pkt.bybit_mid_ref
                        );
                    }
                    RiskDecision::Duplicate | RiskDecision::Skip => {}
                }
                } // end entries allowed
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if last_hb.elapsed() > heartbeat_timeout {
                    safe_mode.on_miss();
                    {
                        let mut w = warm.lock().unwrap();
                        w.entries_halted = operator_halt || safe_mode.halt_entries();
                        w.day_pnl_pct = if deposit > 0.0 {
                            ledger.net_pnl() / deposit
                        } else {
                            0.0
                        };
                        w.publish();
                    }
                    if safe_mode.phase != SafeModePhase::Normal {
                        warn!(
                            "heartbeat miss phase={:?} consecutive={} last_hb_ts={}",
                            safe_mode.phase, safe_mode.consecutive_misses, last_hb_ts_ns
                        );
                    }
                }

                // Emergency / operator flatten without waiting for Binance tick
                if (safe_mode.close_all() || flatten_request) && !positions.is_empty() {
                    warn!(
                        "flatten positions (safe_mode={} operator={})",
                        safe_mode.close_all(),
                        flatten_request
                    );
                    flatten_request = false;
                    for (_id, pos) in positions.drain() {
                        if let Some(symbol) = id_to_bybit.get(&pos.symbol_id) {
                            let close_side = match pos.side {
                                Side::Long => "Sell",
                                Side::Short => "Buy",
                            };
                            let px = mid_for_symbol(&lags, pos.symbol_id, pos.entry_price);
                            if live_orders {
                                if let Some(api) = &bybit_api {
                                    if let Err(e) = place_reduce_only_close(
                                        api,
                                        symbol,
                                        close_side,
                                        pos.qty_remaining,
                                    )
                                    .await
                                    {
                                        warn!("flatten order failed: {e}");
                                    }
                                }
                            }
                            ledger.record_exit(
                                symbol,
                                close_side,
                                pos.qty_remaining,
                                pos.entry_price,
                                px,
                                fee_rate,
                                sim_ledger,
                                "flatten",
                            );
                        }
                    }
                    let _ = ledger.append_jsonl(&ledger_path);
                }

                let _ = ledger.write_summary("logs/paper_summary.txt");
            }
        }
    }
}
