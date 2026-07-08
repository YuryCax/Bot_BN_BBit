use std::sync::atomic::{AtomicU16, Ordering};

use crate::config::SymbolConfig;

pub const MAX_SYMBOLS: usize = 35;

#[derive(Debug)]
pub struct SymbolRegistry {
    pub slots: [Option<SymbolConfig>; MAX_SYMBOLS],
    pub active_count: AtomicU16,
}

impl Default for SymbolRegistry {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            active_count: AtomicU16::new(0),
        }
    }
}

impl SymbolRegistry {
    pub fn from_symbols(symbols: &[SymbolConfig]) -> Self {
        let mut reg = Self::default();
        for s in symbols {
            let idx = (s.id as usize).saturating_sub(1);
            if idx < MAX_SYMBOLS {
                reg.slots[idx] = Some(s.clone());
                reg.active_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        reg
    }

    pub fn get(&self, id: u16) -> Option<&SymbolConfig> {
        self.slots.get(id as usize - 1)?.as_ref()
    }

    pub fn set_enabled(&mut self, id: u16, enabled: bool) {
        if let Some(slot) = self.slots.get_mut(id as usize - 1).and_then(|s| s.as_mut()) {
            slot.enabled = enabled;
        }
    }
}
