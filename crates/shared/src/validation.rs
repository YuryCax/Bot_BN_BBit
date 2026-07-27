use crate::config::{AppConfig, EdgeProfile, SymbolsFile};
use crate::packet::PACKET_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("{0}")]
    Message(String),
}

pub type ValidationResult = Result<(), ValidationError>;

pub fn validate_startup(
    cfg: &AppConfig,
    symbols: &SymbolsFile,
    edge: &EdgeProfile,
    paper_or_live: bool,
) -> ValidationResult {
    if cfg.network.packet_version != PACKET_VERSION {
        return Err(ValidationError::Message(format!(
            "packet_version mismatch: config={} binary={PACKET_VERSION}",
            cfg.network.packet_version
        )));
    }

    if cfg.take_profit.initial_target_pct < cfg.fees.d_min_net_futures() {
        return Err(ValidationError::Message(
            "initial_target_pct < D_min_net_futures".into(),
        ));
    }

    if !cfg.deployment.spot_enabled {
        for s in &symbols.symbol {
            if s.enabled && s.instrument == "spot" {
                return Err(ValidationError::Message(format!(
                    "spot symbol {} enabled but spot_enabled=false",
                    s.binance
                )));
            }
        }
    }

    if paper_or_live {
        if cfg.deployment.mode.eq_ignore_ascii_case("live")
            && cfg.deployment.allow_unverified_paper
        {
            return Err(ValidationError::Message(
                "allow_unverified_paper cannot be used with mode=live".into(),
            ));
        }
        let skip_edge = cfg.deployment.allow_unverified_paper
            && cfg.deployment.mode.eq_ignore_ascii_case("paper");
        if !skip_edge {
            if edge.meta.status != "pass" {
                return Err(ValidationError::Message(
                    "edge_profile.meta.status must be 'pass' for paper/live".into(),
                ));
            }
            let src = edge.meta.data_source.as_deref().unwrap_or("");
            if src.is_empty() || src.eq_ignore_ascii_case("synthetic") {
                return Err(ValidationError::Message(
                    "edge_profile.meta.data_source must be live/binance_vision (synthetic forbidden for paper/live)"
                        .into(),
                ));
            }
            if edge.meta.research_period_days < 14 {
                return Err(ValidationError::Message(format!(
                    "edge_profile.meta.research_period_days={} < 14 required for paper/live",
                    edge.meta.research_period_days
                )));
            }
            let method = edge.meta.research_method.as_deref().unwrap_or("");
            if method != "l2_vwap" {
                return Err(ValidationError::Message(
                    "edge_profile.meta.research_method must be 'l2_vwap' for paper/live".into(),
                ));
            }
            let any_positive = edge
                .edges
                .values()
                .any(|e| e.net_edge_bps > 0.0 && e.trade_hours_utc.len() >= 3);
            if !any_positive {
                return Err(ValidationError::Message(
                    "edge_profile: need ≥1 symbol with net_edge_bps > 0 and ≥3 trade hours".into(),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;

    fn base_cfg() -> AppConfig {
        AppConfig {
            capital: CapitalConfig {
                initial_futures_deposit_usdt: 300.0,
                initial_spot_deposit_usdt: 0.0,
                risk_per_trade_pct: 0.01,
            },
            deployment: DeploymentConfig {
                mode: "start".into(),
                observer_instance_type: "t3.micro".into(),
                executor_instance_type: "t3.small".into(),
                spot_enabled: false,
                min_futures_pf_for_spot: 1.3,
                start_futures_pairs: vec!["BTCUSDT".into()],
                max_symbols: 35,
                edge_profile_path: "config/edge_profile.toml".into(),
                allow_unverified_paper: false,
            },
            resources: ResourcesConfig {
                depth_enabled: false,
                binance_ws_connections: 1,
                zenoh_publish_hz_cap: 100,
                observer_ram_soft_limit_mib: 400,
                executor_ram_soft_limit_mib: 350,
                log_tick_debug: false,
                tokio_worker_threads: 2,
            },
            lag: LagConfig {
                impulse_min_bps: 5.0,
                lag_min_bps: 3.0,
                follow_through_min: 0.4,
                convergence_exit_ratio: 0.75,
                time_stop_ms: 8000,
                capture_est: 0.6,
                bybit_mid_feed_hz: 50,
                bybit_mid_max_staleness_ms: 200,
                bybit_mid_feed_timeout_ms: 500,
            },
            safe_mode: SafeModeConfig {
                heartbeat_miss_caution: 1,
                heartbeat_miss_defensive: 3,
                heartbeat_miss_emergency: 5,
                heartbeat_emergency_timeout_ms: 500,
            },
            fees: FeesConfig {
                spot_maker_pct: 0.001,
                spot_taker_pct: 0.001,
                futures_maker_pct: 0.0002,
                futures_taker_pct: 0.00055,
                fee_profit_buffer_pct: 0.0003,
            },
            execution: ExecutionConfig {
                default_leverage_futures: 3,
                max_leverage_futures: 5,
                margin_mode: "isolated".into(),
                slippage_limit_pct: 0.0005,
                max_slippage_bps: 8.0,
                max_adverse_move_bps: 15.0,
                use_limit_fallback: true,
                limit_fallback_timeout_ms: 50,
                limit_offset_pct: 0.0001,
                slippage_adaptive_resize: true,
            },
            risk: RiskConfig {
                max_daily_drawdown_spot: 0.02,
                max_daily_drawdown_futures: 0.015,
                correlation_limit_btc_eth: 0.6,
                correlation_limit_eth_when_btc_open: 0.3,
                atr_multiplier_stop: 1.8,
                atr_min_filter: 0.002,
            },
            take_profit: TakeProfitConfig {
                enabled: true,
                initial_target_pct: 0.005,
                partial_close_pct: 0.5,
                trail_arm_pct: 0.003,
                sl_breakeven_pct: 0.003,
                sl_tighten_pct: 0.0015,
                base_tp_trail_atr: 1.0,
                extended_trend_tp: true,
                extended_trend_z_min: 2.0,
            },
            signals: SignalsConfig {
                z_score_entry: 2.5,
                z_score_exit: 0.5,
                velocity_min: 0.0001,
            },
            network: NetworkConfig {
                max_latency_ms: 150,
                heartbeat_interval_ms: 100,
                heartbeat_timeout_ms: 500,
                safe_mode_latency_p95_ms: 150,
                packet_version: PACKET_VERSION,
                seq_gap_pause_threshold: 10,
                seq_gap_pause_duration_sec: 5,
            },
            control_panel: ControlPanelConfig {
                enabled: true,
                bind_addr: "127.0.0.1:8080".into(),
                auth_mode: "jwt".into(),
                jwt_secret_env: "PANEL_JWT_SECRET".into(),
                default_spot_alloc_pct: 0.05,
                default_futures_alloc_pct: 0.05,
                max_pair_alloc_pct: 0.25,
                ws_push_interval_ms: 2000,
                audit_log_path: "/var/log/bot/panel_audit.jsonl".into(),
            },
        }
    }

    #[test]
    fn rejects_pending_edge_on_live() {
        let cfg = base_cfg();
        let symbols = SymbolsFile { symbol: vec![] };
        let edge = EdgeProfile {
            meta: EdgeMeta {
                generated_at: "".into(),
                research_period_days: 0,
                injected_latency_ms: 150,
                status: "pending".into(),
                research_method: None,
                data_source: None,
            },
            edges: Default::default(),
        };
        assert!(validate_startup(&cfg, &symbols, &edge, true).is_err());
    }

    #[test]
    fn rejects_synthetic_pass_on_paper() {
        let cfg = base_cfg();
        let symbols = SymbolsFile { symbol: vec![] };
        let mut edges = std::collections::HashMap::new();
        edges.insert(
            "BTCUSDT".into(),
            EdgeSymbolConfig {
                net_edge_bps: 5.0,
                follow_through_min: 0.4,
                lag_min_bps: 3.0,
                trade_hours_utc: vec![13, 14, 15],
                vol_regime_min_atr_pct: 0.0025,
                max_slippage_bps: Some(8.0),
                max_adverse_move_bps: Some(15.0),
            },
        );
        let edge = EdgeProfile {
            meta: EdgeMeta {
                generated_at: "".into(),
                research_period_days: 14,
                injected_latency_ms: 150,
                status: "pass".into(),
                research_method: Some("l2_vwap".into()),
                data_source: Some("synthetic".into()),
            },
            edges,
        };
        assert!(validate_startup(&cfg, &symbols, &edge, true).is_err());
    }

    #[test]
    fn rejects_short_research_period() {
        let cfg = base_cfg();
        let symbols = SymbolsFile { symbol: vec![] };
        let mut edges = std::collections::HashMap::new();
        edges.insert(
            "BTCUSDT".into(),
            EdgeSymbolConfig {
                net_edge_bps: 5.0,
                follow_through_min: 0.4,
                lag_min_bps: 3.0,
                trade_hours_utc: vec![13, 14, 15],
                vol_regime_min_atr_pct: 0.0025,
                max_slippage_bps: Some(8.0),
                max_adverse_move_bps: Some(15.0),
            },
        );
        let edge = EdgeProfile {
            meta: EdgeMeta {
                generated_at: "".into(),
                research_period_days: 7,
                injected_latency_ms: 150,
                status: "pass".into(),
                research_method: Some("l2_vwap".into()),
                data_source: Some("binance_vision".into()),
            },
            edges,
        };
        assert!(validate_startup(&cfg, &symbols, &edge, true).is_err());
    }

    #[test]
    fn allows_unverified_paper_without_edge() {
        let mut cfg = base_cfg();
        cfg.deployment.mode = "paper".into();
        cfg.deployment.allow_unverified_paper = true;
        let symbols = SymbolsFile { symbol: vec![] };
        let edge = EdgeProfile {
            meta: EdgeMeta {
                generated_at: "".into(),
                research_period_days: 1,
                injected_latency_ms: 150,
                status: "fail".into(),
                research_method: Some("l2_vwap".into()),
                data_source: Some("synthetic".into()),
            },
            edges: Default::default(),
        };
        assert!(validate_startup(&cfg, &symbols, &edge, true).is_ok());
    }
}
