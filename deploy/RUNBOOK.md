# Runbook — deploy & rollback (§9.2)

## Build release

```bash
cargo build --release
install -m 755 target/release/{observer,executor,control-panel,telegram-alerts,replay} /opt/bot/bin/
install -m 644 config/*.toml /etc/bot/
```

## Dual-node

1. Tokyo: `observer.service`
2. Singapore: `executor.service`, `control-panel.service`, `telegram-alerts.service`
3. VPC Peering + Zenoh UDP 7447

## Rollback

1. `systemctl stop executor observer`
2. Replace binaries from previous S3 artifact
3. Restore `/etc/bot/config.toml` snapshot
4. `systemctl start executor observer`

## Live staged (Gate §8.6.8)

- Start with 1% deposit sizing via `risk_per_trade_pct`
- Monitor 7 days before full allocation
