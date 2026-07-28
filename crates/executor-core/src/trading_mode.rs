//! Live vs paper order gating.

/// True only when deployment mode is live — never for paper/dev.
pub fn allow_live_orders(mode: &str) -> bool {
    mode.eq_ignore_ascii_case("live")
}

/// Modes that need edge_profile pass unless allow_unverified_paper.
pub fn requires_edge_gate(mode: &str, allow_unverified_paper: bool) -> bool {
    match mode.to_ascii_lowercase().as_str() {
        "live" => true,
        "paper" => !allow_unverified_paper,
        _ => false, // dev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_never_live_orders() {
        assert!(!allow_live_orders("paper"));
        assert!(!allow_live_orders("dev"));
        assert!(allow_live_orders("live"));
    }

    #[test]
    fn unverified_paper_skips_edge() {
        assert!(!requires_edge_gate("paper", true));
        assert!(requires_edge_gate("paper", false));
        assert!(requires_edge_gate("live", true));
    }
}
