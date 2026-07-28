# Runbook — deploy & rollback (§9.2)

**AWS path:** [AWS_QUICKSTART.md](AWS_QUICKSTART.md) (`package_release.ps1` → `install.sh`).

## Build release

```bash
cargo build --release
# Or from Windows: .\scripts\package_release.ps1
install -m 755 target/release/{observer,executor,control-panel,telegram-alerts,replay,smoke-bybit} /opt/bot/bin/
install -m 644 config/*.toml /etc/bot/
# Absolute edge path for systemd WorkingDirectory=/opt/bot:
#   edge_profile_path = "/etc/bot/edge_profile.toml"
```

## Secrets layout

1. Copy `deploy/secrets.env.example` → `/etc/bot/secrets.env` (`chmod 600`, owner `bot`).
2. Fill `BYBIT_*`, `PANEL_JWT_SECRET`, Telegram vars as needed.
3. Never commit secrets. Units use `EnvironmentFile=-/etc/bot/secrets.env`.

## Zenoh dual-node

1. Tokyo: `deploy/zenoh-tokyo.json5.example` → `/etc/bot/zenoh.json5`
2. Singapore: `deploy/zenoh-singapore.json5.example` with Tokyo private IP → `/etc/bot/zenoh.json5`
3. Units set `BOT_ZENOH_CONFIG=/etc/bot/zenoh.json5`
4. Security group / NACL: allow **TCP 7447** both ways on the peering path
5. Outbound HTTPS 443: Binance Futures (Tokyo), Bybit (Singapore)
6. Smoke checklist: [`scripts/smoke_dual_node_check.md`](../scripts/smoke_dual_node_check.md)

## Dual-node process order

1. Tokyo: `systemctl enable --now observer`
2. Singapore: `systemctl enable --now executor control-panel telegram-alerts`
3. Confirm heartbeat + packet log growth before any `mode=paper|live`

## Health probes

- Panel: `GET /health` → `ok` (and JSON with `last_heartbeat_age_ms` when wired)
- Executor logs: no sustained Emergency heartbeat miss
- `mode=dev` first; paper/live only after edge pass (see `deploy/NO_LIVE_UNTIL_PASS.md`)

## Rollback

1. `systemctl stop executor observer`
2. Replace binaries from previous S3 artifact
3. Restore `/etc/bot/config.toml` snapshot
4. `systemctl start executor observer`

## Live staged (Gate §8.6.8)

- Start with 1% deposit sizing via `risk_per_trade_pct`
- Monitor 7 days before full allocation
- Flatten drill: panel/Telegram FlattenAll → positions closed reduce-only on exchange
