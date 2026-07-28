# Dual-node smoke (`mode=dev`, no orders)

Prerequisites: VPC peering / SG allows TCP 7447 between Tokyo↔Singapore; NTP ok.

1. On both hosts: install release bins to `/opt/bot/bin`, configs to `/etc/bot/`.
2. Tokyo: copy `deploy/zenoh-tokyo.json5.example` → `/etc/bot/zenoh.json5`.
3. Singapore: copy `deploy/zenoh-singapore.json5.example`, set Tokyo private IP → `/etc/bot/zenoh.json5`.
4. Both: `BOT_ZENOH_CONFIG=/etc/bot/zenoh.json5` via systemd unit or secrets.env.
5. Ensure `config.toml` has `mode = "dev"` (edge gate skipped).
6. Start Tokyo `observer`, then Singapore `executor`.
7. Pass criteria (within ~30s):
   - Executor logs show heartbeat (no prolonged `heartbeat miss` / Emergency).
   - `BOT_PACKET_LOG` / `logs/packets.bin` grows on Singapore (ticks arriving).
8. Fail: no packets after 60s → check SG/NACL, Zenoh endpoints, `BOT_ZENOH_CONFIG` path.

Do **not** set `mode=live` or place Bybit orders during this smoke.
