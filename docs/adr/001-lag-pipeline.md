# ADR 001: Lag Pipeline (Observer-Only)

**Status:** Accepted (v2.3)  
**Date:** 2026-07-08  
**Context:** Monetizable edge = open lag between Binance impulse and Bybit catch-up. Incorrect lag calculation causes fee-negative trades.

## Decision

1. **Observer** is the sole calculator of `lag_bps`, `lag_residual_bps`, and lag-related `entry_valid` gates.
2. **Executor** publishes Bybit mid on Zenoh topic `system/bybit_mid/{symbol_id}` at **50 Hz** with `{ bybit_mid, ts_ns, symbol_id }`.
3. **Executor warm merge** into entry packets is **forbidden** — avoids split-brain between nodes.
4. **Fail-closed:** if `bybit_mid` age > `bybit_mid_max_staleness_ms` (200 ms) → `entry_valid = 0`.
5. **Feed loss:** no reverse feed for > `bybit_mid_feed_timeout_ms` (500 ms) → halt all entries + CRITICAL alert.

## Rationale

- Entry decision must use **same timestamp domain** as Binance tick (Observer).
- Stale Bybit mid makes `lag_residual` look open when edge is already gone → paying fees for nothing.
- Executor still runs `MICRO_OK`, `BASIS_OK`, `FEE_EDGE_OK` as execution-time confirmation (second gate).

## Consequences

- Observer depends on reverse Zenoh channel; channel health is a **money-critical** metric.
- Edge Research (§9.0) must validate lag gates with `injected_latency_ms = 150`.
- `MarketStatePacket.packet_version = 3` includes lag fields for audit and convergence exit.

## Alternatives Rejected

| Alternative | Why rejected |
|-------------|--------------|
| Executor recalculates lag on receive | Duplicates entry logic (forbidden §1.2) |
| 10 Hz Bybit feed | Too stale for 150 ms freshness budget |
| Continue trading when feed down | Fee-negative blind entries |
