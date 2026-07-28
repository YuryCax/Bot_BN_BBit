# Runbook — deploy & rollback (§9.2)

## Build release

```bash
cargo build --release
# or: sudo ./deploy/install_linux.sh observer|executor
install -m 755 target/release/{observer,executor,control-panel,telegram-alerts,bybit-smoke} /opt/bot/bin/
install -m 644 config/*.toml /etc/bot/
```

## Dual-node

See **[`DUAL_NODE.md`](DUAL_NODE.md)** (canonical).

1. Singapore: `executor.service` (+ panel/telegram), `BOT_ZENOH_CONFIG` → peer Tokyo private IP.
2. Tokyo: `observer.service`, connect to SG private IP.
3. VPC Peering + **TCP 7447** between private CIDRs.
4. Start order: **executor first**, then observer.

## Testnet API (before any mainnet keys)

```bash
# offline gates
./scripts/testnet_order_harness.sh
# e2e
LIVE_ORDERS=1 BYBIT_TESTNET=1 BYBIT_API_KEY=... BYBIT_API_SECRET=... ./scripts/testnet_order_harness.sh
```

Details: [`TESTNET_HARNESS.md`](TESTNET_HARNESS.md).

## Rollback

1. `systemctl stop executor observer`
2. Replace binaries from previous S3 artifact
3. Restore `/etc/bot/config.toml` snapshot
4. `systemctl start executor` then `observer`

## Live staged (Gate §8.6.8)

- **Blocked while** `edge_profile.meta.status != "pass"` — see [`NO_LIVE_UNTIL_PASS.md`](NO_LIVE_UNTIL_PASS.md)
- Mainnet also needs `BOT_ALLOW_MAINNET=1` and `BYBIT_TESTNET=0`
- Start with 1% deposit sizing via `risk_per_trade_pct` — [`STAGED_LIVE.md`](STAGED_LIVE.md)
