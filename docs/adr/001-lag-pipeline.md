# ADR 001: Lag Pipeline (Observer-Only)

**Status:** Superseded by [ADR-003](003-singapore-entry.md) (v2.4)  
**Date:** 2026-07-08  
**Superseded:** 2026-07-27  

## Historical decision (archived)

1. Observer was the sole calculator of `lag_bps`, `lag_residual_bps`, and `entry_valid`.
2. Executor published Bybit mid on `system/bybit_mid/{symbol_id}` at 50 Hz for Observer lag.
3. Executor warm merge into entry packets was forbidden.

## Why superseded

Deciding in Tokyo on reverse-fed Bybit mid adds one full Tokyo↔Singapore RTT of staleness before the execution hop. With net edge of only a few bps, that architecture is fee-negative in practice. See ADR-003: Singapore owns entry with local Bybit mid; Tokyo forwards raw Binance ticks only.
