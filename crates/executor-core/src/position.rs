use shared::packet::{MarketStatePacket, PositionState, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    LagConvergence,
    TimeStop,
    Invalidation,
    StopLoss,
    TakeProfit,
    SafeMode,
    Manual,
}

pub struct PositionManager {
    pub convergence_ratio: f32,
    pub time_stop_ms: u64,
    pub fee_be_long_mult: f64,
}

impl Default for PositionManager {
    fn default() -> Self {
        Self {
            convergence_ratio: 0.75,
            time_stop_ms: 8000,
            fee_be_long_mult: 1.0011,
        }
    }
}

impl PositionManager {
    pub fn effective_sl_long(&self, sl_pnl: f64, sl_binance: f64, fee_be: f64) -> f64 {
        sl_pnl.max(sl_binance).max(fee_be)
    }

    pub fn update_lag_capture(&self, pos: &mut PositionState, packet: &MarketStatePacket) {
        if pos.entry_impulse_bps.abs() < 1e-6 {
            return;
        }
        let residual_ratio = packet.lag_residual_bps.abs() / pos.entry_impulse_bps.abs();
        pos.lag_capture_ratio = 1.0 - residual_ratio.clamp(0.0, 1.0);
    }

    pub fn check_exit(
        &self,
        pos: &PositionState,
        bybit_mid: f64,
        packet: &MarketStatePacket,
        now_ns: u64,
    ) -> Option<ExitReason> {
        if pos.lag_capture_ratio >= self.convergence_ratio {
            return Some(ExitReason::LagConvergence);
        }
        let elapsed_ms = now_ns.saturating_sub(pos.open_time_ns) / 1_000_000;
        if elapsed_ms > self.time_stop_ms && pos.lag_capture_ratio < 0.3 {
            return Some(ExitReason::TimeStop);
        }
        // Require a material residual/impulse flip. `direction_bias == 0` is common on
        // neutral packets and must not invalidate a position by itself.
        const INVALIDATION_RESIDUAL_BPS: f32 = 2.0;
        match pos.side {
            Side::Long
                if packet.lag_residual_bps <= -INVALIDATION_RESIDUAL_BPS
                    && packet.impulse_bps_100ms < 0.0
                    && packet.velocity < 0.0 =>
            {
                return Some(ExitReason::Invalidation);
            }
            Side::Short
                if packet.lag_residual_bps >= INVALIDATION_RESIDUAL_BPS
                    && packet.impulse_bps_100ms > 0.0
                    && packet.velocity > 0.0 =>
            {
                return Some(ExitReason::Invalidation);
            }
            _ => {}
        }
        if pos.current_stop > 0.0 {
            match pos.side {
                Side::Long if bybit_mid <= pos.current_stop => {
                    return Some(ExitReason::StopLoss);
                }
                Side::Short if bybit_mid >= pos.current_stop => {
                    return Some(ExitReason::StopLoss);
                }
                _ => {}
            }
        }
        if pos.current_tp > 0.0 {
            match pos.side {
                Side::Long if bybit_mid >= pos.current_tp => {
                    return Some(ExitReason::TakeProfit);
                }
                Side::Short if bybit_mid <= pos.current_tp => {
                    return Some(ExitReason::TakeProfit);
                }
                _ => {}
            }
        }
        None
    }

    pub fn fee_be_long(&self, entry: f64) -> f64 {
        entry * self.fee_be_long_mult
    }

    pub fn initial_stops(
        &self,
        side: Side,
        entry: f64,
        atr: f32,
        atr_mult: f64,
        tp_pct: f64,
    ) -> (f64, f64) {
        let stop_dist = (atr as f64).max(entry * 0.001) * atr_mult;
        match side {
            Side::Long => (entry - stop_dist, entry * (1.0 + tp_pct)),
            Side::Short => (entry + stop_dist, entry * (1.0 - tp_pct)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::packet::InstrumentType;

    fn position(side: Side) -> PositionState {
        PositionState {
            id: "1".into(),
            symbol_id: 1,
            side,
            instrument: InstrumentType::Futures,
            entry_price: 100.0,
            qty: 1.0,
            qty_remaining: 1.0,
            current_stop: match side {
                Side::Long => 99.0,
                Side::Short => 101.0,
            },
            current_tp: match side {
                Side::Long => 101.0,
                Side::Short => 99.0,
            },
            sl_phase: 0,
            tp_phase: 0,
            partial_done: false,
            pnl_pct: 0.0,
            open_time_ns: 0,
            entry_impulse_bps: match side {
                Side::Long => 10.0,
                Side::Short => -10.0,
            },
            lag_capture_ratio: 0.0,
            exchange_stop_id: None,
        }
    }

    #[test]
    fn effective_sl_monotonic_long() {
        let pm = PositionManager::default();
        let sl = pm.effective_sl_long(100.0, 101.0, 100.5);
        assert!(sl >= 100.5);
    }

    #[test]
    fn convergence_triggers_exit() {
        let pm = PositionManager::default();
        let mut pos = position(Side::Long);
        pos.lag_capture_ratio = 0.8;
        let pkt = MarketStatePacket::neutral(1, 1, 1);
        assert_eq!(
            pm.check_exit(&pos, 100.5, &pkt, 1_000_000_000),
            Some(ExitReason::LagConvergence)
        );
    }

    #[test]
    fn opposite_sign_residual_does_not_fake_full_capture() {
        let pm = PositionManager::default();
        let mut pos = position(Side::Long);
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.lag_residual_bps = -4.0;
        pm.update_lag_capture(&mut pos, &pkt);
        assert!((pos.lag_capture_ratio - 0.6).abs() < 1e-6);
        assert!(pos.lag_capture_ratio < pm.convergence_ratio);
    }

    #[test]
    fn short_stop_loss_and_take_profit_trigger() {
        let pm = PositionManager::default();
        let pos = position(Side::Short);
        let pkt = MarketStatePacket::neutral(1, 1, 1);
        assert_eq!(
            pm.check_exit(&pos, 101.1, &pkt, 1_000_000),
            Some(ExitReason::StopLoss)
        );
        assert_eq!(
            pm.check_exit(&pos, 98.9, &pkt, 1_000_000),
            Some(ExitReason::TakeProfit)
        );
    }

    #[test]
    fn short_thesis_flip_invalidates() {
        let pm = PositionManager::default();
        let pos = position(Side::Short);
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.lag_residual_bps = 3.0;
        pkt.impulse_bps_100ms = 5.0;
        pkt.velocity = 0.001;
        assert_eq!(
            pm.check_exit(&pos, 100.0, &pkt, 1_000_000),
            Some(ExitReason::Invalidation)
        );
    }

    #[test]
    fn neutral_short_packet_does_not_invalidate() {
        let pm = PositionManager::default();
        let pos = position(Side::Short);
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.velocity = 0.001;
        assert_eq!(pm.check_exit(&pos, 100.0, &pkt, 1_000_000), None);
    }
}
