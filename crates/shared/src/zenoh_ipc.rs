//! Zenoh IPC — §2.2, ADR-001

use postcard::{from_bytes, to_allocvec};
use tracing::warn;
use zenoh::bytes::ZBytes;

use crate::packet::{BybitMidFeed, MarketStatePacket};

pub struct ZenohPublisher {
    session: zenoh::Session,
}

impl ZenohPublisher {
    pub async fn open() -> anyhow::Result<Self> {
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow::anyhow!("zenoh open: {e}"))?;
        Ok(Self { session })
    }

    async fn put(&self, key: &str, payload: Vec<u8>) -> anyhow::Result<()> {
        self.session
            .put(key, ZBytes::from(payload))
            .await
            .map_err(|e| anyhow::anyhow!("zenoh put {key}: {e}"))?;
        Ok(())
    }

    pub async fn publish_packet(&self, packet: &MarketStatePacket) -> anyhow::Result<()> {
        let key = format!("market/binance/{}", packet.symbol_id);
        let bytes = to_allocvec(packet)?;
        self.put(&key, bytes).await
    }

    pub async fn publish_bybit_mid(&self, feed: &BybitMidFeed) -> anyhow::Result<()> {
        let key = format!("system/bybit_mid/{}", feed.symbol_id);
        let bytes = to_allocvec(feed)?;
        self.put(&key, bytes).await
    }

    pub async fn publish_heartbeat(&self, ts_ns: u64) -> anyhow::Result<()> {
        let key = "system/heartbeat/tokyo";
        self.put(&key, ts_ns.to_le_bytes().to_vec()).await
    }
}

pub struct ZenohSubscriber {
    session: zenoh::Session,
}

impl ZenohSubscriber {
    pub async fn open() -> anyhow::Result<Self> {
        let session = zenoh::open(zenoh::Config::default())
            .await
            .map_err(|e| anyhow::anyhow!("zenoh open: {e}"))?;
        Ok(Self { session })
    }

    pub async fn run_packets<F>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(MarketStatePacket) + Send,
    {
        let subscriber = self
            .session
            .declare_subscriber("market/binance/**")
            .await
            .map_err(|e| anyhow::anyhow!("zenoh subscribe packets: {e}"))?;
        loop {
            let sample = subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("zenoh recv packet: {e}"))?;
            let bytes = sample.payload().to_bytes();
            match from_bytes::<MarketStatePacket>(&bytes) {
                Ok(pkt) => handler(pkt),
                Err(e) => warn!("bad packet payload: {e}"),
            }
        }
    }

    pub async fn run_bybit_mid<F>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(BybitMidFeed) + Send,
    {
        let subscriber = self
            .session
            .declare_subscriber("system/bybit_mid/**")
            .await
            .map_err(|e| anyhow::anyhow!("zenoh subscribe bybit_mid: {e}"))?;
        loop {
            let sample = subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("zenoh recv bybit_mid: {e}"))?;
            let bytes = sample.payload().to_bytes();
            match from_bytes::<BybitMidFeed>(&bytes) {
                Ok(feed) => handler(feed),
                Err(e) => warn!("bad bybit_mid payload: {e}"),
            }
        }
    }
}
