# AWS quickstart — Tokyo (observer) + Singapore (executor)

> ## BLOCKER: Linux binaries required
>
> `scripts/package_release.ps1` on Windows packages **Windows `.exe`** binaries.
> They will **NOT run on Ubuntu** — `install.sh` only renames them, it cannot convert them.
> Before any AWS deploy, produce `x86_64-unknown-linux-gnu` binaries one of two ways:
>
> 1. **Build on the server** (simplest): install rustup on the AWS host, clone the repo,
>    `cargo build --release --workspace`, copy binaries to `/opt/bot/bin/`.
>    t3.micro is slow — build on the Singapore t3.small (or a temporary larger instance) and copy to Tokyo.
> 2. **Cross-build from this machine** via WSL2 or Docker (Ubuntu container, same cargo command),
>    then run `package_release.ps1` pointing at the Linux `target/release`.
>
> `package_release.ps1` refuses to pack `.exe` unless you pass `-AllowWindowsBinaries` (debug only).

Goal: put keys in gitignored files, smoke APIs, then deploy the same release to two regions and monetize only after gates.

## 0. Local secrets (Windows)

```powershell
.\scripts\init_secrets.ps1
# edit secrets.env — Bybit TESTNET keys, BYBIT_TESTNET=1
.\scripts\Import-BotSecrets.ps1
```

## 1. Local tests (before AWS spend)

```powershell
# Public feeds + Zenoh mono-node (no orders)
.\scripts\smoke_mono_node.ps1 -Seconds 45

# Authenticated Bybit testnet: open → fill → reduce-only close
.\scripts\smoke_bybit_testnet.ps1

# Real L2 edge (≥14d) — required before paper/live
.\scripts\run_quant_hardening.ps1 -Days 14 -LiveDownload
```

Do **not** set `mode=live` until `edge_profile` has `status=pass`, `data_source` ∈ {live,binance_vision}, ≥14 days.

## 5. Package + remote install

```powershell
.\scripts\package_release.ps1
.\scripts\remote_install.ps1 -Role tokyo -SshHost ubuntu@TOKYO_IP -TarPath dist\bot-release-*.tar.gz
.\scripts\remote_install.ps1 -Role singapore -SshHost ubuntu@SG_IP -PeerIp TOKYO_PRIVATE_IP -TarPath dist\bot-release-*.tar.gz -SecretsPath .\secrets.env
```

## 3. AWS network

| Item | Value |
|------|--------|
| Tokyo | t3.micro (or similar), Ubuntu 22.04+ |
| Singapore | t3.small, Ubuntu 22.04+ |
| Peering | VPC peering / TGW |
| SG | TCP **7447** between peers; egress 443 to Binance (TYO) and Bybit (SIN) |
| NTP | chrony enabled |

## 4. Install

```bash
# on each host
sudo mkdir -p /tmp/bot-rel && sudo tar -xzf /tmp/bot-release-*.tar.gz -C /tmp/bot-rel
# Tokyo:
sudo ROLE=tokyo bash /tmp/bot-rel/install.sh
# Singapore:
sudo ROLE=singapore bash /tmp/bot-rel/install.sh
```

Then:

1. Fill `/etc/bot/secrets.env` (chmod 600, owner bot) — same keys as local testnet first.
2. Singapore: set Tokyo private IP in `/etc/bot/zenoh.json5`.
3. Confirm `/etc/bot/config.toml` has `mode = "dev"`.
4. Start Tokyo `observer`, then Singapore `executor` (+ panel/telegram).
5. Dual-node smoke: [`scripts/smoke_dual_node_check.md`](../scripts/smoke_dual_node_check.md).

## 5. Staged monetization

See [`STAGED_LIVE.md`](STAGED_LIVE.md) and [`NO_LIVE_UNTIL_PASS.md`](NO_LIVE_UNTIL_PASS.md).

Order: testnet orders on SG → edge pass → dual-node `dev` → `paper` → `live` with tiny `risk_per_trade_pct`.

## Rollback

`systemctl stop executor observer` → restore previous tarball + `/etc/bot` snapshot → start.
