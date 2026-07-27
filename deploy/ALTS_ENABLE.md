# Sprint E — Alts (SOL / AVAX)

**Rule:** do **not** enable SOL/AVAX until a **real** L2 batch passes production gates.

## Procedure

1. Ensure BTC/ETH already have `status=pass` on live/binance_vision ≥14d (Sprint A).
2. Re-run with alts included:
   ```powershell
   .\scripts\run_quant_hardening.ps1 -LiveDownload -Days 14
   ```
   `analyze_lead_lag.py` includes SOLUSDT/AVAXUSDT by default and only sets `enabled=true` when:
   - global `status=pass` (real data, ≥14d)
   - that symbol `net_edge_bps > 0` and ≥3 positive hours
3. Confirm `config/symbols.toml` before restarting Observer/Executor.
4. Paper the alt alone before live; live still follows [`STAGED_LIVE.md`](STAGED_LIVE.md) (1% ×3, one pair).

## Out of scope

StatArb / cross-MM — separate product, not this roadmap.
