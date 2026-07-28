# Bot_BN_BBit

Low-latency cross-exchange lead-lag trading: **Binance Futures (signal) → Bybit Perpetual (execution)**.

## Project structure

```
config/           # config.toml, symbols.toml, edge_profile.toml, analyst.toml
crates/
  shared/         # MarketStatePacket, config, validation, zenoh IPC
  observer-core/  # Binance WS, Entry Engine, lag §3.5
  executor-core/  # Risk, Position Manager, Bybit signing
  observer-bin/   # observer service
  executor-bin/   # executor service
  panel/          # Control Panel REST API §8.5
  replay/         # Replay engine §9.1
  telegram-alerts/
research/         # Phase 0 Edge Research
analyst/          # Phase 2 offline advisor
book-collector/   # Phase 2 TimescaleDB
deploy/           # systemd + docker-compose
```

## Build (requires Rust 1.78+)

```bash
cargo build --release
cargo test --all
```

Binaries: `target/release/observer`, `executor`, `control-panel`, `replay`, `telegram-alerts`

## Phase 0 — Edge Research

```bash
pip install -r research/collector/requirements.txt
python research/collector/collector.py --duration-sec 3600
python research/edge_report/analyze.py
```

## Run (mono-node dev)

```bash
export BOT_CONFIG=config/config.toml
export BOT_SYMBOLS=config/symbols.toml
cargo run -p observer-bin
cargo run -p executor-bin
cargo run -p panel
```

## Deploy

- Dual-node: `deploy/systemd/` on t3.micro Tokyo + t3.small Singapore
- TimescaleDB: `docker compose -f deploy/docker-compose.yml up -d`

## Gates

- **Live:** `config/edge_profile.toml` → `meta.status = "pass"`
- **Paper:** replay PF ≥ 1.2, follow-through ≥ 40%
- **Phase 3:** `analyst/phase3/shadow.py` → pf_shadow > pf_actual
