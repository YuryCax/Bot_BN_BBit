use shared::packet::MarketStatePacket;
use postcard::to_allocvec;

pub fn encode_packet(packet: &MarketStatePacket) -> anyhow::Result<Vec<u8>> {
    Ok(to_allocvec(packet)?)
}

pub fn topic_for_symbol(symbol_id: u16) -> String {
    format!("market/binance/{symbol_id}")
}

pub const HEARTBEAT_TOPIC: &str = "system/heartbeat/tokyo";
