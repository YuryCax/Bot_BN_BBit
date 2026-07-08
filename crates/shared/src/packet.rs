pub const PACKET_VERSION: u8 = 3;
pub const MAX_SYMBOLS: usize = 35;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Regime {
    Range = 0,
    Transition = 1,
    Trend = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentType {
    Spot,
    Futures,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct MarketStatePacket {
    pub packet_version: u8,
    pub ts_ns: u64,
    pub seq_num: u32,
    pub symbol_id: u16,
    pub entry_valid: u8,
    pub direction_bias: i8,
    pub regime: u8,
    pub z_score: f32,
    pub z_threshold_used: f32,
    pub velocity: f32,
    pub sigma: f32,
    pub d_exp: f32,
    pub d_min: f32,
    pub ema_50: f64,
    pub ema_200: f64,
    pub atr: f32,
    pub ref_price: f64,
    pub bybit_mid_ref: f64,
    pub lag_bps: f32,
    pub lag_residual_bps: f32,
    pub impulse_bps_100ms: f32,
    pub spread_pct: f32,
    pub volume_usd: f32,
    pub bid_ask_imbalance: f32,
    pub volume_delta_100ms: f32,
}

impl MarketStatePacket {
    pub fn neutral(symbol_id: u16, ts_ns: u64, seq_num: u32) -> Self {
        Self {
            packet_version: PACKET_VERSION,
            ts_ns,
            seq_num,
            symbol_id,
            entry_valid: 0,
            direction_bias: 0,
            regime: Regime::Range as u8,
            z_score: 0.0,
            z_threshold_used: 2.5,
            velocity: 0.0,
            sigma: 0.0,
            d_exp: 0.0,
            d_min: 0.0,
            ema_50: 0.0,
            ema_200: 0.0,
            atr: 0.0,
            ref_price: 0.0,
            bybit_mid_ref: 0.0,
            lag_bps: 0.0,
            lag_residual_bps: 0.0,
            impulse_bps_100ms: 0.0,
            spread_pct: 0.0,
            volume_usd: 0.0,
            bid_ask_imbalance: 0.0,
            volume_delta_100ms: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BybitMidFeed {
    pub symbol_id: u16,
    pub bybit_mid: f64,
    pub ts_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionState {
    pub id: String,
    pub symbol_id: u16,
    pub side: Side,
    pub instrument: InstrumentType,
    pub entry_price: f64,
    pub qty: f64,
    pub qty_remaining: f64,
    pub current_stop: f64,
    pub current_tp: f64,
    pub sl_phase: u8,
    pub tp_phase: u8,
    pub partial_done: bool,
    pub pnl_pct: f64,
    pub open_time_ns: u64,
    pub entry_impulse_bps: f32,
    pub lag_capture_ratio: f32,
    pub exchange_stop_id: Option<String>,
}
