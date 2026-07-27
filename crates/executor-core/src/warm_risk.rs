//! Warm-path risk flag updater (DD / funding / book).

use crate::risk::{set_warm_flags, RiskFlags};

#[derive(Debug, Clone, Default)]
pub struct WarmRiskState {
    pub day_pnl_pct: f64,
    pub max_daily_dd_futures: f64,
    pub funding_rate: f64,
    pub max_funding_rate: f64,
    pub book_depth_usd: f64,
    pub min_book_depth_usd: f64,
    pub entries_halted: bool,
}

impl WarmRiskState {
    pub fn compute_flags(&self) -> RiskFlags {
        let mut bits = RiskFlags::all_futures();
        if self.day_pnl_pct <= -self.max_daily_dd_futures {
            bits = bits.with_cleared(RiskFlags::DD_OK | RiskFlags::ENTRIES_FUTURES_OK);
        }
        if self.funding_rate.abs() > self.max_funding_rate {
            bits = bits.with_cleared(RiskFlags::FUNDING_OK);
        }
        if self.book_depth_usd > 0.0 && self.book_depth_usd < self.min_book_depth_usd {
            bits = bits.with_cleared(RiskFlags::BOOK_OK | RiskFlags::MICRO_OK);
        }
        if self.entries_halted {
            bits = bits.with_cleared(RiskFlags::ENTRIES_FUTURES_OK | RiskFlags::PAUSE_OK);
        }
        bits
    }

    pub fn publish(&self) {
        set_warm_flags(self.compute_flags());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::warm_flags;

    #[test]
    fn dd_clears_entries() {
        let s = WarmRiskState {
            max_daily_dd_futures: 0.015,
            day_pnl_pct: -0.02,
            ..Default::default()
        };
        s.publish();
        assert!(!warm_flags().all_required_futures());
        set_warm_flags(RiskFlags::all_futures());
    }
}
