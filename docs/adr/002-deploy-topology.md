# ADR 002: Deploy Topology (Tokyo micro + Singapore small)

**Status:** Accepted (v2.4)  
**Date:** 2026-07-08  
**Updated:** 2026-07-27 (ADR-003: Observer is thin forwarder)

## Decision

| Node | Region | Instance | Services |
|------|--------|----------|----------|
| Observer | ap-northeast-1 (Tokyo) | **t3.micro** | observer forwarder only (Binance WS → Zenoh ticks) |
| Executor | ap-southeast-1 (Singapore) | **t3.small** | executor (entry+risk+orders), control-panel, telegram-alerts |

Paper and live **must not** run executor+panel+telegram on t3.micro (1 GiB).

## Rationale

- Tokyo proximity to Binance Futures WS reduces **signal transport** latency (source of alpha).
- Singapore proximity to Bybit: **entry decision + execution** co-located with local book (ADR-003).
- Observer is lighter than v2.3 (no Entry Engine, no reverse-mid consumer) — t3.micro remains correct.
- OOM on Executor → missed fills → direct PnL loss.

## Mono-node MVP

Single Singapore process allowed **only for dev/debug** (§2.6). Paper go/no-go metrics from mono-node **cannot** replace dual-node validation for live.

## Scale Path

- RAM pressure → t3.medium / c6a.large
- 10+ pairs, proven PF → dual c7a.xlarge (§2.5 Scale C)
- `taskset` pinning only on c7a.xlarge+ (§2.3 scale-only)

## Alternatives Rejected

| Alternative | Why rejected |
|-------------|--------------|
| 2× t3.micro | Singapore OOM under executor+panel+telegram |
| Single node for live | Worse Binance latency; overstates edge in backtest |
| c7a.xlarge from day 1 | Infra cost eats $300 deposit edge |
