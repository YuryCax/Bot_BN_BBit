use shared::packet::MarketStatePacket;
use shared::packet_log::{read_packets, PacketLogWriter};
use shared::time::utc_now_ns;

#[derive(Debug, Default)]
pub struct ReplayStats {
    pub trades: u32,
    pub wins: u32,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub fees: f64,
}

impl ReplayStats {
    pub fn profit_factor(&self) -> f64 {
        if self.gross_loss.abs() < 1e-12 {
            return if self.gross_profit > 0.0 {
                f64::INFINITY
            } else {
                0.0
            };
        }
        self.gross_profit / self.gross_loss.abs()
    }

    pub fn follow_through_rate(&self) -> f64 {
        if self.trades == 0 {
            return 0.0;
        }
        self.wins as f64 / self.trades as f64
    }

    pub fn net_pnl(&self) -> f64 {
        self.gross_profit + self.gross_loss - self.fees
    }
}

pub struct ReplayEngine {
    pub injected_latency_ms: u64,
    pub stats: ReplayStats,
}

impl ReplayEngine {
    pub fn new(injected_latency_ms: u64) -> Self {
        Self {
            injected_latency_ms,
            stats: ReplayStats::default(),
        }
    }

    pub fn on_packet(&mut self, packet: &MarketStatePacket, pnl_delta: f64) {
        if packet.entry_valid != 1 {
            return;
        }
        self.stats.trades += 1;
        let fee = packet.d_min.max(0.0016) as f64 * 100.0; // approx bps→pnl units
        self.stats.fees += fee;
        let net = pnl_delta - fee * 0.01;
        if net > 0.0 {
            self.stats.wins += 1;
            self.stats.gross_profit += net;
        } else {
            self.stats.gross_loss += net;
        }
    }

    pub fn passes_gate(&self) -> bool {
        self.stats.trades >= 20
            && self.stats.profit_factor() >= 1.2
            && self.stats.follow_through_rate() >= 0.40
    }
}

/// Proxy PnL after injected latency: residual lag decays ~linearly over 150ms window.
fn estimate_pnl(packet: &MarketStatePacket, injected_latency_ms: u64) -> f64 {
    if packet.entry_valid == 0 {
        return 0.0;
    }
    let decay = (injected_latency_ms as f64 / 150.0).clamp(0.0, 1.5);
    let capture = (0.55 - 0.15 * decay).clamp(0.15, 0.55);
    let edge = packet.lag_residual_bps as f64 * capture;
    // Small noise from mid vs bybit ref
    let slip = if packet.bybit_mid_ref > 0.0 && packet.ref_price > 0.0 {
        ((packet.bybit_mid_ref - packet.ref_price) / packet.ref_price * 10_000.0).abs() * 0.3
    } else {
        2.0
    };
    (edge - slip) / 100.0
}

fn write_fixture(path: &str, n: usize) -> anyhow::Result<()> {
    let mut w = PacketLogWriter::open(path)?;
    let now = utc_now_ns();
    for i in 0..n {
        let mut p = MarketStatePacket::neutral(
            1 + (i % 2) as u16,
            now + i as u64 * 100_000_000,
            i as u32 + 1,
        );
        // ~55% winners after latency to pass FT≥40% and PF≥1.2 with enough trades
        let win = i % 5 != 0;
        p.entry_valid = 1;
        p.direction_bias = if i % 2 == 0 { 1 } else { -1 };
        p.lag_residual_bps = if win { 12.0 } else { 4.0 };
        p.impulse_bps_100ms = if win { 8.0 } else { 5.5 };
        p.d_exp = 0.003;
        p.d_min = 0.0016;
        p.ref_price = 67_000.0;
        p.bybit_mid_ref = if win { 66_995.0 } else { 67_010.0 };
        w.append(&p)?;
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let log_path = args
        .iter()
        .find(|a| a.ends_with(".bin") && !a.starts_with('-'))
        .cloned()
        .or_else(|| args.get(1).filter(|a| !a.starts_with('-')).cloned())
        .unwrap_or_else(|| "logs/packets.bin".into());
    let injected_ms: u64 = std::env::var("BOT_REPLAY_LATENCY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);

    let allow_fixture = args.iter().any(|a| a == "--allow-fixture")
        || std::env::var("BOT_REPLAY_ALLOW_FIXTURE").ok().as_deref() == Some("1");

    if args.iter().any(|a| a == "--write-fixture") {
        let n: usize = args
            .iter()
            .position(|a| a == "--write-fixture")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        write_fixture(&log_path, n)?;
        println!("wrote fixture {n} packets -> {log_path} (dev only; not a live gate)");
    }

    let path = std::path::Path::new(&log_path);
    if !path.exists() {
        if allow_fixture {
            eprintln!("no log at {log_path}; generating fixture (--allow-fixture)");
            write_fixture(&log_path, 120)?;
        } else {
            anyhow::bail!(
                "no packets in {log_path}. Run dual-node to capture real logs, \
                 or pass --allow-fixture for unit smoke only."
            );
        }
    }

    let mut engine = ReplayEngine::new(injected_ms);
    let packets = read_packets(&log_path)?;
    if packets.is_empty() {
        anyhow::bail!("no packets in {log_path}");
    }

    for packet in &packets {
        engine.on_packet(packet, estimate_pnl(packet, injected_ms));
    }

    println!(
        "replay file={log_path} packets={} trades={} pf={:.2} ft={:.1}% net={:.4} pass={} allow_fixture={}",
        packets.len(),
        engine.stats.trades,
        engine.stats.profit_factor(),
        engine.stats.follow_through_rate() * 100.0,
        engine.stats.net_pnl(),
        engine.passes_gate(),
        allow_fixture
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_can_pass_gates() {
        let path = std::env::temp_dir().join("bot_replay_fixture.bin");
        write_fixture(path.to_str().unwrap(), 120).unwrap();
        let mut engine = ReplayEngine::new(150);
        for p in read_packets(&path).unwrap() {
            engine.on_packet(&p, estimate_pnl(&p, 150));
        }
        assert!(engine.stats.trades >= 20);
        assert!(
            engine.passes_gate(),
            "pf={} ft={}",
            engine.stats.profit_factor(),
            engine.stats.follow_through_rate()
        );
    }
}
