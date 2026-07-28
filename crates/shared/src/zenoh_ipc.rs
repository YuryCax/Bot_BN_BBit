//! Zenoh IPC — ADR-003 (Tokyo forwarder / Singapore entry)

use postcard::{from_bytes, to_allocvec};
use tracing::warn;
use zenoh::bytes::ZBytes;

use crate::packet::{BinanceTick, BybitMidFeed, MarketStatePacket, OperatorCommand};

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

    /// ADR-003: raw Binance tick forward (preferred).
    pub async fn publish_binance_tick(&self, tick: &BinanceTick) -> anyhow::Result<()> {
        let key = format!("binance/tick/{}", tick.symbol_id);
        let bytes = to_allocvec(tick)?;
        self.put(&key, bytes).await
    }

    /// Legacy MarketStatePacket publish (audit / mono-node local log path).
    pub async fn publish_packet(&self, packet: &MarketStatePacket) -> anyhow::Result<()> {
        let key = format!("market/binance/{}", packet.symbol_id);
        let bytes = to_allocvec(packet)?;
        self.put(&key, bytes).await
    }

    /// Optional audit reverse mid — not used for entry under ADR-003.
    pub async fn publish_bybit_mid(&self, feed: &BybitMidFeed) -> anyhow::Result<()> {
        let key = format!("system/bybit_mid/{}", feed.symbol_id);
        let bytes = to_allocvec(feed)?;
        self.put(&key, bytes).await
    }

    pub async fn publish_heartbeat(&self, ts_ns: u64) -> anyhow::Result<()> {
        let key = "system/heartbeat/tokyo";
        self.put(&key, ts_ns.to_le_bytes().to_vec()).await
    }

    pub async fn publish_command(&self, cmd: &OperatorCommand) -> anyhow::Result<()> {
        let bytes = to_allocvec(cmd)?;
        self.put("system/command", bytes).await
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

    pub async fn run_binance_ticks<F>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(BinanceTick) + Send,
    {
        let subscriber = self
            .session
            .declare_subscriber("binance/tick/**")
            .await
            .map_err(|e| anyhow::anyhow!("zenoh subscribe ticks: {e}"))?;
        loop {
            let sample = subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("zenoh recv tick: {e}"))?;
            let bytes = sample.payload().to_bytes();
            match from_bytes::<BinanceTick>(&bytes) {
                Ok(tick) => handler(tick),
                Err(e) => warn!("bad BinanceTick payload: {e}"),
            }
        }
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

    pub async fn run_heartbeat<F>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(u64) + Send,
    {
        let subscriber = self
            .session
            .declare_subscriber("system/heartbeat/**")
            .await
            .map_err(|e| anyhow::anyhow!("zenoh subscribe heartbeat: {e}"))?;
        loop {
            let sample = subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("zenoh recv heartbeat: {e}"))?;
            let bytes = sample.payload().to_bytes();
            if bytes.len() >= 8 {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[..8]);
                handler(u64::from_le_bytes(arr));
            }
        }
    }

    pub async fn run_commands<F>(&self, mut handler: F) -> anyhow::Result<()>
    where
        F: FnMut(OperatorCommand) + Send,
    {
        let subscriber = self
            .session
            .declare_subscriber("system/command")
            .await
            .map_err(|e| anyhow::anyhow!("zenoh subscribe command: {e}"))?;
        loop {
            let sample = subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("zenoh recv command: {e}"))?;
            let bytes = sample.payload().to_bytes();
            match from_bytes::<OperatorCommand>(&bytes) {
                Ok(cmd) => handler(cmd),
                Err(e) => warn!("bad OperatorCommand payload: {e}"),
            }
        }
    }
}
