# Bot_BN_BBit

Low-latency cross-exchange lead-lag: **Binance Futures (Tokyo signal) → Bybit Perp (Singapore execution)**.

## Operator path (keys → tests → AWS → monetization)

```powershell
# 1) Secrets (gitignored)
.\scripts\init_secrets.ps1
# edit secrets.env — Bybit TESTNET keys, BYBIT_TESTNET=1

# 2) Wiring smoke (public WS, no orders)
.\scripts\smoke_mono_node.ps1

# 3) Authenticated Bybit testnet order lifecycle
.\scripts\Import-BotSecrets.ps1
.\scripts\smoke_bybit_testnet.ps1

# 4) Real edge research (≥14d L2) before paper/live
.\scripts\run_quant_hardening.ps1 -Days 14 -LiveDownload

# 5) Package + AWS (scp+ssh one-shot)
.\scripts\package_release.ps1
.\scripts\remote_install.ps1 -Role tokyo -SshHost ubuntu@TOKYO -TarPath dist\bot-release-XXXX.tar.gz
.\scripts\remote_install.ps1 -Role singapore -SshHost ubuntu@SG -PeerIp TOKYO_PRIV -TarPath dist\bot-release-XXXX.tar.gz -SecretsPath .\secrets.env
```

Deploy details: **[`deploy/AWS_QUICKSTART.md`](deploy/AWS_QUICKSTART.md)**  
Gates: [`deploy/NO_LIVE_UNTIL_PASS.md`](deploy/NO_LIVE_UNTIL_PASS.md) · staged live: [`deploy/STAGED_LIVE.md`](deploy/STAGED_LIVE.md)

Keep `mode = "dev"` until edge `status=pass` on real L2. Never commit `secrets.env`.

## Project structure

```
config/           # config.toml, symbols.toml, edge_profile.toml
crates/           # observer / executor / panel / smoke-bybit / …
research/         # Edge research (quant + collector)
deploy/           # systemd, install.sh, AWS quickstart
scripts/          # smoke, package, secrets helpers
secrets.env.example
```

## Build

```bash
cargo build --release
cargo test --workspace --all-targets
```

## Run (mono-node dev)

```powershell
.\scripts\Import-BotSecrets.ps1   # if secrets.env present
.\scripts\run_mono_node.ps1
```

## Deploy summary

| Host | Services |
|------|----------|
| Tokyo | `observer` |
| Singapore | `executor`, `control-panel`, `telegram-alerts` |

Zenoh TCP 7447 between peers; secrets in `/etc/bot/secrets.env`.
