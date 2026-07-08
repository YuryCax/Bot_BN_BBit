use std::sync::atomic::{AtomicU64, Ordering};

use shared::packet::MarketStatePacket;
use shared::time::utc_now_ns;

#[derive(Clone, Copy, Debug, Default)]
pub struct RiskFlags(u64);

impl RiskFlags {
    pub const CAPITAL_OK: u64 = 1 << 0;
    pub const DD_OK: u64 = 1 << 1;
    pub const CORR_OK: u64 = 1 << 2;
    pub const BOOK_OK: u64 = 1 << 3;
    pub const FUNDING_OK: u64 = 1 << 4;
    pub const BASIS_OK: u64 = 1 << 5;
    pub const MICRO_OK: u64 = 1 << 6;
    pub const PAUSE_OK: u64 = 1 << 8;
    pub const PAIR_ENABLED: u64 = 1 << 9;
    pub const ENTRIES_FUTURES_OK: u64 = 1 << 11;
    pub const FEE_EDGE_OK: u64 = 1 << 12;

    pub const ALL_FUTURES: u64 = Self::CAPITAL_OK
        | Self::DD_OK
        | Self::CORR_OK
        | Self::BOOK_OK
        | Self::FUNDING_OK
        | Self::BASIS_OK
        | Self::MICRO_OK
        | Self::PAUSE_OK
        | Self::PAIR_ENABLED
        | Self::ENTRIES_FUTURES_OK
        | Self::FEE_EDGE_OK;

    pub fn all_futures() -> Self {
        Self(Self::ALL_FUTURES)
    }

    pub fn contains_all(&self, required: u64) -> bool {
        self.0 & required == required
    }

    pub fn bits(self) -> u64 {
        self.0
    }

    pub fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub fn all_required_futures(self) -> bool {
        self.contains_all(Self::ALL_FUTURES)
    }
}

static WARM_FLAGS: AtomicU64 = AtomicU64::new(RiskFlags::ALL_FUTURES);

pub fn set_warm_flags(flags: RiskFlags) {
    WARM_FLAGS.store(flags.bits(), Ordering::Release);
}

pub fn warm_flags() -> RiskFlags {
    RiskFlags::from_bits(WARM_FLAGS.load(Ordering::Acquire))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDecision {
    Open,
    Skip,
    Stale,
    Duplicate,
}

pub struct RiskEngine {
    pub max_latency_ns: u64,
    last_seq: [u32; shared::registry::MAX_SYMBOLS],
}

impl Default for RiskEngine {
    fn default() -> Self {
        Self {
            max_latency_ns: 150_000_000,
            last_seq: [0; shared::registry::MAX_SYMBOLS],
        }
    }
}

impl RiskEngine {
    pub fn check_entry(&mut self, packet: &MarketStatePacket) -> RiskDecision {
        if packet.entry_valid == 0 {
            return RiskDecision::Skip;
        }
        let latency = utc_now_ns().saturating_sub(packet.ts_ns);
        if latency > self.max_latency_ns {
            return RiskDecision::Stale;
        }
        let idx = packet.symbol_id as usize;
        if idx == 0 || idx > shared::registry::MAX_SYMBOLS {
            return RiskDecision::Skip;
        }
        let slot = idx - 1;
        if packet.seq_num <= self.last_seq[slot] {
            return RiskDecision::Duplicate;
        }
        self.last_seq[slot] = packet.seq_num;
        if !warm_flags().all_required_futures() {
            return RiskDecision::Skip;
        }
        if packet.d_exp < packet.d_min {
            return RiskDecision::Skip;
        }
        RiskDecision::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::packet::MarketStatePacket;

    #[test]
    fn rejects_entry_valid_zero() {
        let mut re = RiskEngine::default();
        let mut p = MarketStatePacket::neutral(1, utc_now_ns(), 1);
        p.entry_valid = 0;
        assert_eq!(re.check_entry(&p), RiskDecision::Skip);
    }
}
