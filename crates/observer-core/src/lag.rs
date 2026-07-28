use shared::packet::BybitMidFeed;
use shared::time::utc_now_ns;

#[derive(Debug, Clone)]
pub struct LagState {
    pub bybit_mid: f64,
    pub bybit_ts_ns: u64,
    pub binance_mid_100ms_ago: f64,
    pub max_staleness_ms: u64,
}

impl Default for LagState {
    fn default() -> Self {
        Self {
            bybit_mid: 0.0,
            bybit_ts_ns: 0,
            binance_mid_100ms_ago: 0.0,
            max_staleness_ms: 200,
        }
    }
}

impl LagState {
    pub fn apply_feed(&mut self, feed: &BybitMidFeed) {
        self.bybit_mid = feed.bybit_mid;
        self.bybit_ts_ns = feed.ts_ns;
    }

    pub fn is_stale(&self) -> bool {
        if self.bybit_ts_ns == 0 {
            return true;
        }
        let age_ms = utc_now_ns().saturating_sub(self.bybit_ts_ns) / 1_000_000;
        age_ms > self.max_staleness_ms
    }

    pub fn lag_bps(&self, binance_mid: f64) -> f32 {
        if self.bybit_mid <= 0.0 {
            return 0.0;
        }
        ((binance_mid - self.bybit_mid) / self.bybit_mid * 10_000.0) as f32
    }

    pub fn impulse_bps(&self, binance_mid: f64) -> f32 {
        if self.binance_mid_100ms_ago <= 0.0 {
            return 0.0;
        }
        ((binance_mid - self.binance_mid_100ms_ago) / self.binance_mid_100ms_ago * 10_000.0)
            as f32
    }

    /// Signed residual: impulse not yet reflected in Bybit mid (negative on downside lag).
    pub fn lag_residual_bps(&self, binance_mid: f64) -> f32 {
        let impulse = self.impulse_bps(binance_mid);
        if self.bybit_mid <= 0.0 || self.binance_mid_100ms_ago <= 0.0 {
            return 0.0;
        }
        let bybit_move =
            (self.bybit_mid - self.binance_mid_100ms_ago) / self.binance_mid_100ms_ago * 10_000.0;
        impulse - bybit_move as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_side_residual_is_negative_when_bybit_lags_dump() {
        let state = LagState {
            bybit_mid: 99.5,
            bybit_ts_ns: 1,
            binance_mid_100ms_ago: 100.0,
            max_staleness_ms: 200,
        };
        // Binance dumped to 99.0; Bybit only at 99.5 → residual negative magnitude
        let r = state.lag_residual_bps(99.0);
        assert!(r < 0.0, "expected downside residual, got {r}");
        assert!(r.abs() >= 3.0);
    }
}
