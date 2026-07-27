# ADR 003: Singapore Entry (Tokyo Forwarder)

**Status:** Accepted (v2.4)  
**Date:** 2026-07-27  
**Supersedes:** [ADR-001](001-lag-pipeline.md)

## Context

Lead-lag edge on BTC/ETH is measured in a few bps after fees. Under ADR-001 the Observer decided in Tokyo using a reverse-fed Bybit mid (already aged by one Tokyo↔Singapore hop), then sent `entry_valid` to Singapore for another hop before the order. That double-hop burned more latency budget than the edge.

## Decision

1. **Tokyo Observer is a thin forwarder only:** Binance Futures WS → parse → publish raw tick on Zenoh `binance/tick/{symbol_id}` (`BinanceTick`: mid, ts_ns, seq, symbol_id). It does **not** compute `entry_valid`.
2. **Singapore Executor owns Entry Engine §7:** local Bybit mid/book + forwarded Binance tick → `LagState` / `EntryEngine` → Risk → order.
3. **Reverse `system/bybit_mid` is not required for entry** (optional audit/mono-node only).
4. **Fail-closed:** no Binance tick within freshness budget OR no local Bybit mid → no entry.
5. **Injected latency for research** = age of forwarded Binance tick at decide time (not reverse-mid RTT).

## Latency budget

| Segment | Typical |
|---------|---------|
| Binance → Tokyo Observer | 10–30 ms |
| Tokyo → Singapore (raw tick) | 50–80 ms P95 |
| Local Bybit mid at decide | ~0–5 ms |
| Order RTT Bybit | ~1–5 ms |

Total decide→fill is one backbone hop + local book, not decide-on-stale-mid + second hop.

## Consequences

- `MarketStatePacket` may still be logged locally on Executor for replay/audit after entry evaluation.
- Observer t3.micro becomes even lighter (ADR-002 still valid).
- Spec §1.2 / §3.5 / §7 host moves to Executor (ТЗ v2.4).
- Split-brain avoided because **Tokyo never decides**.

## Alternatives Rejected

| Alternative | Why rejected |
|-------------|--------------|
| Keep ADR-001 Observer-only entry | Reverse mid ages lag truth past monetizable edge |
| Python/Redis rewrite | Regression vs Rust+Zenoh; no earn benefit |
| Single-node live | Overstates Binance latency; forbidden for live go/no-go |
| Executor recalculating while Observer also sets entry_valid | Split-brain |
