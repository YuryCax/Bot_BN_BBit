#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeModePhase {
    Normal,
    Caution,
    Defensive,
    Emergency,
}

pub struct SafeMode {
    pub miss_caution: u32,
    pub miss_defensive: u32,
    pub miss_emergency: u32,
    pub consecutive_misses: u32,
    pub phase: SafeModePhase,
}

impl SafeMode {
    pub fn new(miss_caution: u32, miss_defensive: u32, miss_emergency: u32) -> Self {
        Self {
            miss_caution,
            miss_defensive,
            miss_emergency,
            consecutive_misses: 0,
            phase: SafeModePhase::Normal,
        }
    }

    pub fn on_heartbeat(&mut self) {
        self.consecutive_misses = 0;
        self.phase = SafeModePhase::Normal;
    }

    pub fn on_miss(&mut self) {
        self.consecutive_misses += 1;
        self.phase = if self.consecutive_misses >= self.miss_emergency {
            SafeModePhase::Emergency
        } else if self.consecutive_misses >= self.miss_defensive {
            SafeModePhase::Defensive
        } else if self.consecutive_misses >= self.miss_caution {
            SafeModePhase::Caution
        } else {
            SafeModePhase::Normal
        };
    }

    pub fn halt_entries(&self) -> bool {
        matches!(
            self.phase,
            SafeModePhase::Caution | SafeModePhase::Defensive | SafeModePhase::Emergency
        )
    }

    pub fn close_all(&self) -> bool {
        self.phase == SafeModePhase::Emergency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase1_does_not_close() {
        let mut sm = SafeMode::new(1, 3, 5);
        sm.on_miss();
        assert_eq!(sm.phase, SafeModePhase::Caution);
        assert!(!sm.close_all());
    }
}
