use shared::config::LagConfig;
use shared::packet::{MarketStatePacket, Regime};
use shared::time::utc_now_ns;

use crate::math::SymbolMetrics;

pub struct EntryEngine {
    pub z_threshold: f64,
    pub velocity_min: f64,
    pub d_min_net: f64,
    pub alpha: f32,
    pub beta: f32,
    pub delta_t: f32,
    pub lag_min_bps: f32,
    pub impulse_min_bps: f32,
    pub capture_est: f32,
}

impl EntryEngine {
    pub fn from_config(lag: &LagConfig, d_min_net: f64, z_threshold: f64, velocity_min: f64) -> Self {
        Self {
            z_threshold,
            velocity_min,
            d_min_net,
            alpha: 0.4,
            beta: 0.6,
            delta_t: 0.3,
            lag_min_bps: lag.lag_min_bps,
            impulse_min_bps: lag.impulse_min_bps,
            capture_est: lag.capture_est,
        }
    }

    pub fn evaluate(
        &self,
        metrics: &SymbolMetrics,
        mid: f64,
        lag_residual_bps: f32,
        impulse_bps: f32,
        bybit_stale: bool,
        trade_hour_ok: bool,
    ) -> MarketStatePacket {
        let ts_ns = utc_now_ns();
        let z = metrics.welford.z_score(mid) as f32;
        let vel = crate::math::velocity(mid, metrics.price_100ms_ago(10)) as f32;
        let sigma = metrics.welford.sigma() as f32;
        let mut d_exp = self.alpha * z.abs() * sigma + self.beta * vel.abs() * self.delta_t;
        if vel < 0.0 && d_exp > 0.0 {
            d_exp *= 0.7;
        }
        let d_min = self.d_min_net as f32;
        let regime = metrics.regime();

        let mut entry_valid = 0u8;
        let mut direction_bias = 0i8;

        let lag_open = lag_residual_bps >= self.lag_min_bps;
        let impulse_ok = impulse_bps.abs() >= self.impulse_min_bps;
        let edge_ok = d_exp >= d_min && (lag_residual_bps * self.capture_est) >= d_min;

        if !bybit_stale && trade_hour_ok && lag_open && impulse_ok && edge_ok {
            let z_ok = z.abs() >= self.z_threshold as f32;
            if z_ok {
                if vel > self.velocity_min as f32
                    && metrics.ema_50 > metrics.ema_200
                    && self.regime_allows_long(regime)
                {
                    entry_valid = 1;
                    direction_bias = 1;
                } else if vel < -(self.velocity_min as f32)
                    && metrics.ema_50 < metrics.ema_200
                    && self.regime_allows_short(regime)
                {
                    entry_valid = 1;
                    direction_bias = -1;
                }
            }
        }

        MarketStatePacket {
            packet_version: shared::packet::PACKET_VERSION,
            ts_ns,
            seq_num: 0,
            symbol_id: 0,
            entry_valid,
            direction_bias,
            regime,
            z_score: z,
            z_threshold_used: self.z_threshold as f32,
            velocity: vel,
            sigma,
            d_exp,
            d_min,
            ema_50: metrics.ema_50,
            ema_200: metrics.ema_200,
            atr: metrics.atr as f32,
            ref_price: mid,
            bybit_mid_ref: 0.0,
            lag_bps: 0.0,
            lag_residual_bps,
            impulse_bps_100ms: impulse_bps,
            spread_pct: 0.0,
            volume_usd: 0.0,
            bid_ask_imbalance: 0.0,
            volume_delta_100ms: 0.0,
        }
    }

    fn regime_allows_long(&self, regime: u8) -> bool {
        matches!(regime, 0 | 1) || regime == Regime::Trend as u8
    }

    fn regime_allows_short(&self, regime: u8) -> bool {
        matches!(regime, 0 | 1) || regime == Regime::Trend as u8
    }
}
