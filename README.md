# Bot_BN_BBit

Low-latency lead-lag: **Binance Futures (Tokyo forward) → Bybit Perp (Singapore entry)** — ADR-003.

## Architecture (v2.4)

| Node | Role |
|------|------|
| **Observer** (Tokyo) | Thin forwarder: `BinanceTick` + heartbeat — **no** `entry_valid` |
| **Executor** (Singapore) | Local Bybit mid + Entry §7 + Risk (`max_adverse_move_bps` ≠ book slip) + orders |

## Build

```bash
cargo build --release -p observer-bin -p executor-bin
cargo test -p shared -p executor-core -p observer-core -p replay
```

## Gates before paper/live

1. `.\scripts\run_quant_hardening.ps1 -LiveDownload` → `edge_profile` `status=pass`, `data_source!=synthetic`, `research_period_days≥14`
2. Dual-node paper + `logs/paper_ledger.jsonl`; replay **real** `logs/packets.bin` (no auto-fixture)
3. Staged live checklist: [`deploy/STAGED_LIVE.md`](deploy/STAGED_LIVE.md) — 1% risk, ×3, one pair
4. SOL/AVAX: [`deploy/ALTS_ENABLE.md`](deploy/ALTS_ENABLE.md) — stay off until real L2 pass

## Dev mono-node

```powershell
.\scripts\smoke_mono_node.ps1    # timed wiring smoke (dev only)
.\scripts\run_mono_node.ps1      # interactive (Ctrl+C)
```

Requires `mode=dev` until real edge pass. Not valid for live go/no-go.

**Do not trade / do not rent dual-node for money while fail:** [`deploy/NO_LIVE_UNTIL_PASS.md`](deploy/NO_LIVE_UNTIL_PASS.md)

## Product (Phase 1)

See [`deploy/PRODUCT.md`](deploy/PRODUCT.md).

- **Paper never sends live orders** (`mode!=live`).
- Kill switch: Panel `/api/v1/trading/halt` + Telegram `/pause` → Zenoh → Executor.
- SL/TP set on open; Stop-Market on live.
- Edge still `fail` on real hist — do not set `mode=live`.
