use shared::config::TakeProfitConfig;
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
    PartialTakeProfit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailAction {
    None,
    /// Close this qty (reduce-only); caller updates qty_remaining / partial_done.
    PartialClose,
    LevelsUpdated,
}

#[derive(Debug, Clone)]
pub struct TrailResult {
    pub action: TrailAction,
    pub stop_changed: bool,
    pub partial_qty: f64,
}

pub struct PositionManager {
    pub convergence_ratio: f32,
    pub time_stop_ms: u64,
    /// Round-trip fee multiplier for long BE (entry * mult).
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
    pub fn from_fees(futures_taker_pct: f64, fee_buffer_pct: f64) -> Self {
        let rt = 2.0 * futures_taker_pct + fee_buffer_pct.max(0.0);
        Self {
            fee_be_long_mult: 1.0 + rt,
            ..Default::default()
        }
    }

    pub fn fee_be_long(&self, entry: f64) -> f64 {
        entry * self.fee_be_long_mult
    }

    pub fn fee_be_short(&self, entry: f64) -> f64 {
        entry / self.fee_be_long_mult
    }

    pub fn effective_sl_long(&self, sl_pnl: f64, sl_binance: f64, fee_be: f64) -> f64 {
        sl_pnl.max(sl_binance).max(fee_be)
    }

    pub fn effective_sl_short(&self, sl_pnl: f64, sl_binance: f64, fee_be: f64) -> f64 {
        sl_pnl.min(sl_binance).min(fee_be)
    }

    pub fn update_lag_capture(&self, pos: &mut PositionState, packet: &MarketStatePacket) {
        if pos.entry_impulse_bps.abs() < 1e-6 {
            return;
        }
        let ratio = packet.lag_residual_bps.abs() / pos.entry_impulse_bps.abs();
        pos.lag_capture_ratio = 1.0 - ratio.clamp(0.0, 1.0);
    }

    pub fn pnl_pct(side: Side, entry: f64, mid: f64) -> f64 {
        if entry <= 0.0 {
            return 0.0;
        }
        match side {
            Side::Long => (mid - entry) / entry,
            Side::Short => (entry - mid) / entry,
        }
    }

    /// Ratchet SL/TP toward monetization; never loosens protective stop.
    pub fn update_dynamic_levels(
        &self,
        pos: &mut PositionState,
        bybit_mid: f64,
        atr: f32,
        tp_cfg: &TakeProfitConfig,
    ) -> TrailResult {
        if !tp_cfg.enabled || bybit_mid <= 0.0 || pos.entry_price <= 0.0 {
            return TrailResult {
                action: TrailAction::None,
                stop_changed: false,
                partial_qty: 0.0,
            };
        }

        let pnl = Self::pnl_pct(pos.side, pos.entry_price, bybit_mid);
        pos.pnl_pct = pnl;
        let old_stop = pos.current_stop;
        let atr_abs = if atr > 0.0 {
            atr as f64
        } else {
            pos.entry_price * 0.003
        };

        // SL phases
        if pnl >= tp_cfg.sl_breakeven_pct {
            pos.sl_phase = 2;
            let fee_be = match pos.side {
                Side::Long => self.fee_be_long(pos.entry_price),
                Side::Short => self.fee_be_short(pos.entry_price),
            };
            match pos.side {
                Side::Long => {
                    pos.current_stop = self.effective_sl_long(pos.current_stop, fee_be, fee_be);
                }
                Side::Short => {
                    pos.current_stop = self.effective_sl_short(pos.current_stop, fee_be, fee_be);
                }
            }
        } else if pnl >= tp_cfg.sl_tighten_pct {
            pos.sl_phase = pos.sl_phase.max(1);
            match pos.side {
                Side::Long => {
                    let tight = pos.entry_price * (1.0 - tp_cfg.sl_tighten_pct * 0.25);
                    pos.current_stop = pos.current_stop.max(tight.min(bybit_mid));
                }
                Side::Short => {
                    let tight = pos.entry_price * (1.0 + tp_cfg.sl_tighten_pct * 0.25);
                    pos.current_stop = pos.current_stop.min(tight.max(bybit_mid));
                }
            }
        }

        // Trail arm: ratchet protective stop with price; extend TP target
        if pnl >= tp_cfg.trail_arm_pct {
            pos.tp_phase = pos.tp_phase.max(1);
            let trail_dist = atr_abs * tp_cfg.base_tp_trail_atr.max(0.1);
            match pos.side {
                Side::Long => {
                    let trail_sl = bybit_mid - trail_dist;
                    pos.current_stop = pos.current_stop.max(trail_sl);
                    pos.current_tp = pos.current_tp.max(bybit_mid + trail_dist * 0.5);
                }
                Side::Short => {
                    let trail_sl = bybit_mid + trail_dist;
                    pos.current_stop = pos.current_stop.min(trail_sl);
                    let new_tp = bybit_mid - trail_dist * 0.5;
                    pos.current_tp = if pos.current_tp > 0.0 {
                        pos.current_tp.min(new_tp).max(bybit_mid * 0.001)
                    } else {
                        new_tp.max(bybit_mid * 0.001)
                    };
                }
            }
        }

        let stop_changed = (pos.current_stop - old_stop).abs() > 1e-8;

        // Partial TP at initial target
        if !pos.partial_done && pnl >= tp_cfg.initial_target_pct && tp_cfg.partial_close_pct > 0.0 {
            let qty = pos.qty_remaining * tp_cfg.partial_close_pct.clamp(0.05, 0.95);
            if qty > 0.0 {
                return TrailResult {
                    action: TrailAction::PartialClose,
                    stop_changed,
                    partial_qty: qty,
                };
            }
        }

        TrailResult {
            action: if stop_changed {
                TrailAction::LevelsUpdated
            } else {
                TrailAction::None
            },
            stop_changed,
            partial_qty: 0.0,
        }
    }

    pub fn check_exit(
        &self,
        pos: &PositionState,
        bybit_mid: f64,
        packet: &MarketStatePacket,
        now_ns: u64,
    ) -> Option<ExitReason> {
        if !bybit_mid.is_finite() || bybit_mid <= 0.0 {
            return None; // no price → do not act on exits (wait)
        }
        if pos.lag_capture_ratio >= self.convergence_ratio {
            return Some(ExitReason::LagConvergence);
        }
        let elapsed_ms = now_ns.saturating_sub(pos.open_time_ns) / 1_000_000;
        if elapsed_ms > self.time_stop_ms && pos.lag_capture_ratio < 0.3 {
            return Some(ExitReason::TimeStop);
        }
        // Thesis invalidation: residual + impulse flipped against position.
        // Do NOT use direction_bias==0 (evaluate returns 0 on most ticks → false exits).
        const INV_RES_BPS: f32 = 2.0;
        match pos.side {
            Side::Long => {
                if packet.lag_residual_bps <= -INV_RES_BPS
                    && packet.impulse_bps_100ms < 0.0
                    && packet.velocity < 0.0
                {
                    return Some(ExitReason::Invalidation);
                }
            }
            Side::Short => {
                if packet.lag_residual_bps >= INV_RES_BPS
                    && packet.impulse_bps_100ms > 0.0
                    && packet.velocity > 0.0
                {
                    return Some(ExitReason::Invalidation);
                }
            }
        }
        if pos.current_stop > 0.0 {
            if pos.side == Side::Long && bybit_mid <= pos.current_stop {
                return Some(ExitReason::StopLoss);
            }
            if pos.side == Side::Short && bybit_mid >= pos.current_stop {
                return Some(ExitReason::StopLoss);
            }
        }
        if pos.current_tp > 0.0 {
            if pos.side == Side::Long && bybit_mid >= pos.current_tp {
                return Some(ExitReason::TakeProfit);
            }
            if pos.side == Side::Short && bybit_mid <= pos.current_tp {
                return Some(ExitReason::TakeProfit);
            }
        }
        None
    }

    /// Initial local SL/TP. Protective SL is never tighter than round-trip fee distance
    /// (fee margin): avoids fee-sized noise stops with no analytical edge left.
    pub fn initial_stops(
        &self,
        side: Side,
        entry: f64,
        atr: f32,
        atr_mult_sl: f64,
        tp_pct: f64,
    ) -> (f64, f64) {
        let atr_abs = if atr > 0.0 {
            atr as f64
        } else {
            entry * 0.003
        };
        let mut sl_dist = atr_abs * atr_mult_sl;
        // Min SL distance ≥ fee RT in price space (fee_be_long_mult - 1)
        let fee_dist = entry * (self.fee_be_long_mult - 1.0).max(0.0);
        if fee_dist > 0.0 {
            sl_dist = sl_dist.max(fee_dist);
        }
        let tp_dist = entry * tp_pct.max(0.0001);
        match side {
            Side::Long => {
                let mut sl = entry - sl_dist;
                if sl >= entry {
                    sl = entry - fee_dist.max(entry * 0.001);
                }
                (sl, entry + tp_dist)
            }
            Side::Short => {
                let mut sl = entry + sl_dist;
                if sl <= entry {
                    sl = entry + fee_dist.max(entry * 0.001);
                }
                (sl, (entry - tp_dist).max(entry * 0.001))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::packet::InstrumentType;

    fn tp_cfg() -> TakeProfitConfig {
        TakeProfitConfig {
            enabled: true,
            initial_target_pct: 0.005,
            partial_close_pct: 0.5,
            trail_arm_pct: 0.003,
            sl_breakeven_pct: 0.003,
            sl_tighten_pct: 0.0015,
            base_tp_trail_atr: 1.0,
            extended_trend_tp: true,
            extended_trend_z_min: 2.0,
        }
    }

    fn pos(side: Side, entry: f64, stop: f64, tp: f64) -> PositionState {
        PositionState {
            id: "1".into(),
            symbol_id: 1,
            side,
            instrument: InstrumentType::Futures,
            entry_price: entry,
            qty: 1.0,
            qty_remaining: 1.0,
            current_stop: stop,
            current_tp: tp,
            sl_phase: 0,
            tp_phase: 0,
            partial_done: false,
            pnl_pct: 0.0,
            open_time_ns: 0,
            entry_impulse_bps: 10.0,
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
    fn long_stop_only_ratchets_up() {
        let pm = PositionManager::default();
        let mut p = pos(Side::Long, 100.0, 99.0, 101.0);
        let r = pm.update_dynamic_levels(&mut p, 100.4, 1.0, &tp_cfg());
        assert!(p.current_stop >= 99.0);
        let _ = r;
        let stop_after_be = p.current_stop;
        let _ = pm.update_dynamic_levels(&mut p, 100.1, 1.0, &tp_cfg());
        assert!(p.current_stop >= stop_after_be - 1e-9);
    }

    #[test]
    fn short_stop_only_ratchets_down() {
        let pm = PositionManager::default();
        let mut p = pos(Side::Short, 100.0, 101.0, 99.0);
        let _ = pm.update_dynamic_levels(&mut p, 99.6, 1.0, &tp_cfg());
        assert!(p.current_stop <= 101.0);
        let after = p.current_stop;
        let _ = pm.update_dynamic_levels(&mut p, 99.9, 1.0, &tp_cfg());
        assert!(p.current_stop <= after + 1e-9);
    }

    #[test]
    fn fee_be_short_below_entry() {
        let pm = PositionManager::from_fees(0.00055, 0.0003);
        assert!(pm.fee_be_short(100.0) < 100.0);
        assert!(pm.fee_be_long(100.0) > 100.0);
    }

    #[test]
    fn partial_triggers_at_target() {
        let pm = PositionManager::default();
        let mut p = pos(Side::Long, 100.0, 99.0, 105.0);
        let r = pm.update_dynamic_levels(&mut p, 100.6, 1.0, &tp_cfg());
        assert_eq!(r.action, TrailAction::PartialClose);
        assert!(r.partial_qty > 0.0);
    }

    #[test]
    fn convergence_triggers_exit() {
        let pm = PositionManager::default();
        let mut p = pos(Side::Long, 100.0, 99.0, 101.0);
        p.lag_capture_ratio = 0.8;
        let pkt = MarketStatePacket::neutral(1, 1, 1);
        assert_eq!(
            pm.check_exit(&p, 100.5, &pkt, 1_000_000_000),
            Some(ExitReason::LagConvergence)
        );
    }

    #[test]
    fn initial_stops_long_below_entry() {
        let pm = PositionManager::default();
        let (sl, tp) = pm.initial_stops(Side::Long, 100.0, 1.0, 1.8, 0.005);
        assert!(sl < 100.0);
        assert!(tp > 100.0);
    }

    #[test]
    fn stop_loss_triggers_long() {
        let pm = PositionManager::default();
        let p = pos(Side::Long, 100.0, 99.0, 105.0);
        let pkt = MarketStatePacket::neutral(1, 1, 1);
        assert_eq!(
            pm.check_exit(&p, 98.5, &pkt, 1_000_000),
            Some(ExitReason::StopLoss)
        );
    }

    #[test]
    fn capture_uses_abs_residual() {
        let pm = PositionManager::default();
        let mut p = pos(Side::Short, 100.0, 101.0, 99.0);
        p.entry_impulse_bps = -10.0;
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.lag_residual_bps = -4.0;
        pm.update_lag_capture(&mut p, &pkt);
        assert!((p.lag_capture_ratio - 0.6).abs() < 1e-3);
    }

    #[test]
    fn neutral_packet_does_not_false_invalidate() {
        let pm = PositionManager::default();
        let p = pos(Side::Long, 100.0, 99.0, 105.0);
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.velocity = -0.001;
        pkt.direction_bias = 0;
        pkt.lag_residual_bps = 1.0;
        pkt.impulse_bps_100ms = 5.0;
        assert_eq!(pm.check_exit(&p, 100.2, &pkt, 1_000_000), None);
    }

    #[test]
    fn residual_flip_invalidates_long() {
        let pm = PositionManager::default();
        let p = pos(Side::Long, 100.0, 99.0, 105.0);
        let mut pkt = MarketStatePacket::neutral(1, 1, 1);
        pkt.velocity = -0.001;
        pkt.lag_residual_bps = -5.0;
        pkt.impulse_bps_100ms = -8.0;
        assert_eq!(
            pm.check_exit(&p, 100.2, &pkt, 1_000_000),
            Some(ExitReason::Invalidation)
        );
    }
}
