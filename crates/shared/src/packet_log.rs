use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

use postcard::{from_bytes, to_allocvec};

use crate::packet::MarketStatePacket;

const MAX_RECORD_BYTES: usize = 4096;

pub struct PacketLogWriter {
    file: File,
}

impl PacketLogWriter {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        Ok(Self { file })
    }

    pub fn append(&mut self, packet: &MarketStatePacket) -> io::Result<()> {
        let bytes = to_allocvec(packet).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packet too large for log",
            ));
        }
        let len = bytes.len() as u32;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&bytes)?;
        Ok(())
    }
}

pub fn read_packets(path: impl AsRef<Path>) -> anyhow::Result<Vec<MarketStatePacket>> {
    let mut file = File::open(path.as_ref())?;
    let mut out = Vec::new();
    loop {
        let mut len_buf = [0u8; 4];
        match file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        if len == 0 || len > MAX_RECORD_BYTES {
            anyhow::bail!("invalid record length {len}");
        }
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf)?;
        out.push(from_bytes::<MarketStatePacket>(&buf)?);
    }
    Ok(out)
}
