//! Rolling follow-through rate for Observer entry gate (§9.0 / edge_profile).

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct FollowThroughTracker {
    pub min_rate: f32,
    window: VecDeque<bool>,
    capacity: usize,
}

impl FollowThroughTracker {
    pub fn new(min_rate: f32, capacity: usize) -> Self {
        Self {
            min_rate,
            window: VecDeque::with_capacity(capacity),
            capacity: capacity.max(10),
        }
    }

    pub fn record(&mut self, aligned: bool) {
        if self.window.len() >= self.capacity {
            self.window.pop_front();
        }
        self.window.push_back(aligned);
    }

    pub fn rate(&self) -> f32 {
        if self.window.is_empty() {
            return 1.0; // fail-open until we have samples
        }
        let wins = self.window.iter().filter(|&&a| a).count();
        wins as f32 / self.window.len() as f32
    }

    pub fn allows_entry(&self) -> bool {
        // Require at least 20 samples before enforcing
        if self.window.len() < 20 {
            return true;
        }
        self.rate() >= self.min_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_when_ft_low() {
        let mut ft = FollowThroughTracker::new(0.4, 50);
        for _ in 0..30 {
            ft.record(false);
        }
        assert!(!ft.allows_entry());
    }

    #[test]
    fn allows_when_ft_high() {
        let mut ft = FollowThroughTracker::new(0.4, 50);
        for _ in 0..30 {
            ft.record(true);
        }
        assert!(ft.allows_entry());
    }
}
