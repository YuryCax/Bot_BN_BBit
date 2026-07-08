# Bot_BN_BBit

Low-latency cross-exchange lead-lag trading: **Binance Futures (signal) → Bybit Perpetual (execution)**.

**Primary goal:** positive **net edge** after fees — not observation, not LLM hype.

## Status

| Phase | State |
|-------|-------|
| **0 — Edge Research** | **Start here** — `research/` |
| **1 — Rust bot + Panel** | Not started |
| **2 — Analyst + DB** | Not started |
| **3 — Validated Analyst** | Not started |

## Quick Start

1. Read the spec: [`бот на Rust4.md`](бот%20на%20Rust4.md) (v2.3)
2. Run §9.0 Edge Research → fill `config/edge_profile.toml`
3. Only then scaffold Rust workspace (§2.7.3)

## Configuration

| File | Purpose |
|------|---------|
| [`config/config.toml`](config/config.toml) | Main bot config (`packet_version = 3`) |
| [`config/symbols.toml`](config/symbols.toml) | MVP futures pairs (BTC, ETH) |
| [`config/edge_profile.toml`](config/edge_profile.toml) | **Required for live** — from Edge Research |

Copy [`.env.example`](.env.example) → `.env` for secrets (never commit).

## Architecture (summary)

```
Binance WS → Observer (Tokyo) → Entry Engine + lag gates
                    ↓ Zenoh UDP
              Executor (Singapore) → Risk → Bybit
                    ↑ bybit_mid feed (50 Hz)
```

See ADRs: [`docs/adr/`](docs/adr/)

## Infrastructure (paper/live)

- **Tokyo** `t3.micro` — Observer
- **Singapore** `t3.small` — Executor + Panel + Telegram

## Hard Rules

- No live without §9.0 pass (`net_edge_bps > 0`)
- No LLM in hot path
- Observer decides entry; Executor does not recalculate Z/D_exp/lag
- Stale `bybit_mid` → no entry (fail-closed)

## License

Private — trading system. Do not commit API keys.
