use postcard::from_bytes;
use shared::packet::MarketStatePacket;

pub fn decode_packet(bytes: &[u8]) -> anyhow::Result<MarketStatePacket> {
    Ok(from_bytes::<MarketStatePacket>(bytes)?)
}

pub fn freshness_ok(packet: &MarketStatePacket, max_latency_ns: u64) -> bool {
    let now = shared::time::utc_now_ns();
    now.saturating_sub(packet.ts_ns) <= max_latency_ns
}
