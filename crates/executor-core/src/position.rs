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
        let captured = (packet.lag_residual_bps / pos.entry_impulse_bps).clamp(0.0, 1.0);
        pos.lag_capture_ratio = 1.0 - captured;
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
        if pos.side == Side::Long && packet.velocity < 0.0 && packet.direction_bias <= 0 {
            return Some(ExitReason::Invalidation);
        }
        if pos.side == Side::Long && bybit_mid <= pos.current_stop {
            return Some(ExitReason::StopLoss);
        }
        if pos.side == Side::Long && bybit_mid >= pos.current_tp {
            return Some(ExitReason::TakeProfit);
        }
        None
    }

    pub fn fee_be_long(&self, entry: f64) -> f64 {
        entry * self.fee_be_long_mult
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::packet::InstrumentType;

    #[test]
    fn effective_sl_monotonic_long() {
        let pm = PositionManager::default();
        let sl = pm.effective_sl_long(100.0, 101.0, 100.5);
        assert!(sl >= 100.5);
    }

    #[test]
    fn convergence_triggers_exit() {
        let pm = PositionManager::default();
        let pos = PositionState {
            id: "1".into(),
            symbol_id: 1,
            side: Side::Long,
            instrument: InstrumentType::Futures,
            entry_price: 100.0,
            qty: 1.0,
            qty_remaining: 1.0,
            current_stop: 99.0,
            current_tp: 101.0,
            sl_phase: 0,
            tp_phase: 0,
            partial_done: false,
            pnl_pct: 0.0,
            open_time_ns: 0,
            entry_impulse_bps: 10.0,
            lag_capture_ratio: 0.8,
            exchange_stop_id: None,
        };
        let pkt = MarketStatePacket::neutral(1, 1, 1);
        assert_eq!(
            pm.check_exit(&pos, 100.5, &pkt, 1_000_000_000),
            Some(ExitReason::LagConvergence)
        );
    }
}
