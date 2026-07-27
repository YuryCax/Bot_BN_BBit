//! Paper / dry-run ledger — fees, fills, net PnL (Sprint C).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shared::time::utc_now_ns;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerFill {
    pub ts_ns: u64,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub notional: f64,
    pub fee_usdt: f64,
    pub is_entry: bool,
    pub dry_run: bool,
    pub note: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PaperLedger {
    pub fills: Vec<LedgerFill>,
    pub realized_pnl_usdt: f64,
    pub fees_usdt: f64,
    pub open_entries: u32,
    pub closed_trades: u32,
}

impl PaperLedger {
    pub fn record_entry(
        &mut self,
        symbol: &str,
        side: &str,
        qty: f64,
        price: f64,
        fee_rate: f64,
        dry_run: bool,
    ) {
        let notional = qty * price;
        let fee = notional * fee_rate;
        self.fees_usdt += fee;
        self.open_entries += 1;
        self.fills.push(LedgerFill {
            ts_ns: utc_now_ns(),
            symbol: symbol.into(),
            side: side.into(),
            qty,
            price,
            notional,
            fee_usdt: fee,
            is_entry: true,
            dry_run,
            note: "entry".into(),
        });
    }

    pub fn record_exit(
        &mut self,
        symbol: &str,
        side: &str,
        qty: f64,
        entry_price: f64,
        exit_price: f64,
        fee_rate: f64,
        dry_run: bool,
        note: &str,
    ) {
        let notional = qty * exit_price;
        let fee = notional * fee_rate;
        self.fees_usdt += fee;
        // side here is close side; PnL from entry
        let dir = if side.eq_ignore_ascii_case("Sell") {
            1.0
        } else {
            -1.0
        };
        // If we Sell to close Long: pnl = (exit - entry) * qty
        let pnl = dir * (exit_price - entry_price) * qty;
        self.realized_pnl_usdt += pnl - fee;
        self.closed_trades += 1;
        self.fills.push(LedgerFill {
            ts_ns: utc_now_ns(),
            symbol: symbol.into(),
            side: side.into(),
            qty,
            price: exit_price,
            notional,
            fee_usdt: fee,
            is_entry: false,
            dry_run,
            note: note.into(),
        });
    }

    pub fn net_pnl(&self) -> f64 {
        self.realized_pnl_usdt
    }

    pub fn append_jsonl(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        if let Some(last) = self.fills.last() {
            writeln!(f, "{}", serde_json::to_string(last).unwrap_or_default())?;
        }
        Ok(())
    }

    pub fn write_summary(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = format!(
            "closed_trades={}\nfees_usdt={:.4}\nnet_pnl_usdt={:.4}\nfills={}\n",
            self.closed_trades,
            self.fees_usdt,
            self.net_pnl(),
            self.fills.len()
        );
        std::fs::write(path, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_pnl() {
        let mut led = PaperLedger::default();
        led.record_entry("BTCUSDT", "Buy", 0.01, 100.0, 0.00055, true);
        led.record_exit("BTCUSDT", "Sell", 0.01, 100.0, 101.0, 0.00055, true, "tp");
        assert!(led.net_pnl() > 0.0);
        assert_eq!(led.closed_trades, 1);
    }
}
