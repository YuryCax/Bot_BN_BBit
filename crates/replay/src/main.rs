use shared::packet::MarketStatePacket;
use shared::packet_log::read_packets;

#[derive(Debug, Default)]
pub struct ReplayStats {
    pub trades: u32,
    pub wins: u32,
    pub gross_pnl: f64,
    pub fees: f64,
}

impl ReplayStats {
    pub fn profit_factor(&self) -> f64 {
        if self.fees >= self.gross_pnl {
            0.0
        } else {
            self.gross_pnl / self.fees.max(1e-9)
        }
    }

    pub fn follow_through_rate(&self) -> f64 {
        if self.trades == 0 {
            return 0.0;
        }
        self.wins as f64 / self.trades as f64
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
        if packet.entry_valid == 1 {
            self.stats.trades += 1;
            if pnl_delta > 0.0 {
                self.stats.wins += 1;
            }
            self.stats.gross_pnl += pnl_delta;
            self.stats.fees += packet.d_min as f64 * 100.0;
        }
    }

    pub fn passes_gate(&self) -> bool {
        self.stats.profit_factor() >= 1.2 && self.stats.follow_through_rate() >= 0.40
    }
}

fn estimate_pnl(packet: &MarketStatePacket) -> f64 {
    if packet.entry_valid == 0 {
        return 0.0;
    }
    // Proxy: capture fraction of lag residual after injected latency penalty
    let latency_penalty = 0.15;
    packet.lag_residual_bps as f64 * (1.0 - latency_penalty) / 100.0
}

fn main() -> anyhow::Result<()> {
    let log_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "logs/packets.bin".into());
    let injected_ms: u64 = std::env::var("BOT_REPLAY_LATENCY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);

    let mut engine = ReplayEngine::new(injected_ms);
    let packets = read_packets(&log_path)?;
    if packets.is_empty() {
        anyhow::bail!("no packets in {log_path}");
    }

    for packet in &packets {
        engine.on_packet(packet, estimate_pnl(packet));
    }

    println!(
        "replay file={log_path} packets={} pf={:.2} ft={:.1}% pass={}",
        packets.len(),
        engine.stats.profit_factor(),
        engine.stats.follow_through_rate() * 100.0,
        engine.passes_gate()
    );
    Ok(())
}
