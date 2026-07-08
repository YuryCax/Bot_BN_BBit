# Edge Research (§9.0) — Phase 0

**Goal:** Prove `net_edge_bps > 0` after fees and 150 ms latency **before** any paper/live trading.

Live is **forbidden** while `config/edge_profile.toml` has `status = "pending"` or `net_edge_bps <= 0`.

## Workflow

1. **Collect** (1–2 weeks, 2–3 futures pairs)
   - `binance_mid`, `bybit_mid` every 100 ms
   - `impulse_bps_100ms`, forward returns Bybit at +200/+500/+1000 ms
   - Output: `research/data/*.parquet` (hourly checkpoints + final flush)

2. **Analyze**
   - Follow-through rate by `(symbol, hour_utc, vol_bucket)`
   - Conditional return after impulse ≥ `impulse_min_bps`
   - `net_edge_bps = conditional_return - fee_round_trip - slippage (0.05%)`
   - Output: `research/edge_report/` (notebook, charts, summary.md)

3. **Publish thresholds**
   - Update `config/edge_profile.toml` with measured values
   - Set `status = "pass"` only if ≥1 symbol has `net_edge_bps > 0` in ≥3 hourly windows

## Go / No-Go

| Pass | Fail |
|------|------|
| net_edge_bps > 0 on ≥1 pair | Stop or change pairs/hours |
| edge_profile.toml generated | Do not start Rust paper bot |
| trade_hours_utc from data | Do not guess follow_through_min |

## Tools

- Phase 0 collector: Python (recommended) or Observer §3.5 telemetry
- Analysis: pandas + Jupyter
- Inject latency 150 ms in replay before trusting live PF

## Directory Layout

```
research/
├── README.md           # this file
├── data/               # raw Parquet (gitignored)
├── edge_report/        # analysis artifacts
└── collector/          # (TODO) standalone collector script
```
