use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub capital: CapitalConfig,
    pub deployment: DeploymentConfig,
    pub resources: ResourcesConfig,
    pub lag: LagConfig,
    pub fees: FeesConfig,
    pub safe_mode: SafeModeConfig,
    pub execution: ExecutionConfig,
    pub risk: RiskConfig,
    pub take_profit: TakeProfitConfig,
    pub signals: SignalsConfig,
    pub network: NetworkConfig,
    pub control_panel: ControlPanelConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SafeModeConfig {
    pub heartbeat_miss_caution: u32,
    pub heartbeat_miss_defensive: u32,
    pub heartbeat_miss_emergency: u32,
    pub heartbeat_emergency_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CapitalConfig {
    pub initial_futures_deposit_usdt: f64,
    pub initial_spot_deposit_usdt: f64,
    pub risk_per_trade_pct: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeploymentConfig {
    pub mode: String,
    pub observer_instance_type: String,
    pub executor_instance_type: String,
    pub spot_enabled: bool,
    pub min_futures_pf_for_spot: f64,
    pub start_futures_pairs: Vec<String>,
    pub max_symbols: usize,
    pub edge_profile_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourcesConfig {
    pub depth_enabled: bool,
    pub binance_ws_connections: u32,
    pub zenoh_publish_hz_cap: u32,
    pub observer_ram_soft_limit_mib: u32,
    pub executor_ram_soft_limit_mib: u32,
    pub log_tick_debug: bool,
    pub tokio_worker_threads: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LagConfig {
    pub impulse_min_bps: f32,
    pub lag_min_bps: f32,
    pub follow_through_min: f32,
    pub convergence_exit_ratio: f32,
    pub time_stop_ms: u64,
    pub capture_est: f32,
    pub bybit_mid_feed_hz: u32,
    pub bybit_mid_max_staleness_ms: u64,
    pub bybit_mid_feed_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeesConfig {
    pub spot_maker_pct: f64,
    pub spot_taker_pct: f64,
    pub futures_maker_pct: f64,
    pub futures_taker_pct: f64,
    pub fee_profit_buffer_pct: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionConfig {
    pub default_leverage_futures: u32,
    pub max_leverage_futures: u32,
    pub margin_mode: String,
    pub slippage_limit_pct: f64,
    pub use_limit_fallback: bool,
    pub limit_fallback_timeout_ms: u64,
    pub limit_offset_pct: f64,
    pub slippage_adaptive_resize: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RiskConfig {
    pub max_daily_drawdown_spot: f64,
    pub max_daily_drawdown_futures: f64,
    pub correlation_limit_btc_eth: f64,
    pub correlation_limit_eth_when_btc_open: f64,
    pub atr_multiplier_stop: f64,
    pub atr_min_filter: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TakeProfitConfig {
    pub enabled: bool,
    pub initial_target_pct: f64,
    pub partial_close_pct: f64,
    pub trail_arm_pct: f64,
    pub sl_breakeven_pct: f64,
    pub sl_tighten_pct: f64,
    pub base_tp_trail_atr: f64,
    pub extended_trend_tp: bool,
    pub extended_trend_z_min: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignalsConfig {
    pub z_score_entry: f64,
    pub z_score_exit: f64,
    pub velocity_min: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    pub max_latency_ms: u64,
    pub heartbeat_interval_ms: u64,
    pub heartbeat_timeout_ms: u64,
    pub safe_mode_latency_p95_ms: u64,
    pub packet_version: u8,
    pub seq_gap_pause_threshold: u32,
    pub seq_gap_pause_duration_sec: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ControlPanelConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub auth_mode: String,
    pub jwt_secret_env: String,
    pub default_spot_alloc_pct: f64,
    pub default_futures_alloc_pct: f64,
    pub max_pair_alloc_pct: f64,
    pub ws_push_interval_ms: u64,
    pub audit_log_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SymbolConfig {
    pub id: u16,
    pub binance: String,
    pub bybit: String,
    pub instrument: String,
    pub leverage: Option<u32>,
    pub enabled: bool,
    pub futures_alloc_pct: Option<f64>,
    pub spot_alloc_pct: Option<f64>,
    pub spot_margin_enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SymbolsFile {
    pub symbol: Vec<SymbolConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeMeta {
    pub generated_at: String,
    pub research_period_days: u32,
    pub injected_latency_ms: u32,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeSymbolConfig {
    pub net_edge_bps: f64,
    pub follow_through_min: f64,
    pub lag_min_bps: f64,
    pub trade_hours_utc: Vec<u32>,
    pub vol_regime_min_atr_pct: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeProfile {
    pub meta: EdgeMeta,
    #[serde(flatten)]
    pub edges: std::collections::HashMap<String, EdgeSymbolConfig>,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

impl SymbolsFile {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

impl EdgeProfile {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let value: toml::Value = toml::from_str(&text)?;
        let meta: EdgeMeta = value
            .get("meta")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing [meta]"))?
            .try_into()?;
        let mut edges = std::collections::HashMap::new();
        if let Some(edge_table) = value.get("edge").and_then(|v| v.as_table()) {
            for (sym, v) in edge_table {
                let cfg: EdgeSymbolConfig = v.clone().try_into()?;
                edges.insert(sym.clone(), cfg);
            }
        }
        Ok(Self { meta, edges })
    }
}

impl FeesConfig {
    pub fn d_min_net_futures(&self) -> f64 {
        self.futures_taker_pct * 2.0 + 0.0005 + self.fee_profit_buffer_pct
    }
}
