# Edge Research (§9.0) + Phase 0.5 Quant Hardening (ТЗ v2.4)

**Goal:** Prove `net_edge_bps > 0` after fees, **forwarded Binance tick age ~150 ms**, and **L2 VWAP slippage** before paper/live.

## Production pass rules (hard)

`config/edge_profile.toml` may set `status = "pass"` **only if all** hold:

| Rule | Requirement |
|------|-------------|
| `research_method` | `l2_vwap` |
| `data_source` | `live` or `binance_vision` — **never** `synthetic` for production |
| `research_period_days` | **≥ 14** |
| Per symbol | `net_edge_bps > 0` and ≥3 positive `trade_hours_utc` |

**Synthetic fixtures are for CI / offline tooling only.** They must write `status = "fail"` or `data_source = "synthetic"` so paper/live validation rejects them.

## Slippage semantics

| Field | Meaning | Used for |
|-------|---------|----------|
| `max_slippage_bps` / `book_slippage_bps` | L2 VWAP cost at target notional | Edge research, sizing |
| `max_adverse_move_bps` | Max Bybit mid move vs signal ref after decide | Executor kill (not equal to book slip) |

Using book VWAP (~0.5 bps) as the adverse-move kill incorrectly rejects every open-lag signal (`lag_min_bps ≥ 3`).

## Workflow

```powershell
.\scripts\run_quant_hardening.ps1 -LiveDownload   # production candidate
.\scripts\run_quant_hardening.ps1                 # synthetic → must not unlock live
.\scripts\run_r1_research.ps1                     # R1: event-time + fee realism + sweep (existing hist)
```

## R1 notes

- Fee model: **Bybit-only** round-trip taker (~11 bps). Binance is signal-only.
- Primary measure: **event-time asof** (`--method event`); bar-join kept for comparison.
- Sweep output: `research/edge_report/sweep_heatmap.csv`, `sweep_report.md`.
- Do **not** rent AWS dual-node for trading or set `mode=paper/live` while `status=fail`.

## Directory Layout

```
research/
├── README.md
├── data/               # mid collector parquet
├── data/hist/          # Phase 0.5 hist trades/L2
├── edge_report/        # summary, sweep, params_for_rust.json
├── collector/          # mid collector §9.0
└── quant/              # download_hist, ccf_lag, orderbook_sim, analyze_lead_lag, sweep_edge
```
