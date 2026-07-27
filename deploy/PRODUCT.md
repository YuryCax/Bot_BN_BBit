# Product runbook (Phase 1 / ADR-003)

## What you have

| Binary | Role |
|--------|------|
| `observer` | Tokyo: Binance tick + heartbeat |
| `executor` | Singapore: entry + risk + SL/TP + ledger |
| `control-panel` | HTTP halt → Zenoh `system/command` |
| `telegram-alerts` | `/pause` `/resume` `/flatten` `/status` → same bus |

## Modes

| mode | Edge pass required | Real Bybit orders |
|------|--------------------|-------------------|
| `dev` | no | **never** |
| `paper` | yes, unless `allow_unverified_paper=true` | **never** |
| `live` | always | yes (needs API keys) |

## Local product smoke

```powershell
.\scripts\smoke_mono_node.ps1 -Seconds 60
```

Paper sim without edge (ledger only, no live orders):

```toml
# config/config.toml
mode = "paper"
allow_unverified_paper = true
```

Then `.\scripts\run_mono_node.ps1` — watch `logs/paper_ledger.jsonl`.

## Kill switch

```bash
curl -X POST http://127.0.0.1:8080/api/v1/trading/halt \
  -H "content-type: application/json" \
  -d "{\"wallet\":\"futures\",\"halt_entries\":true,\"flatten\":false}"
```

Telegram (with `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID`): `/pause` `/resume` `/flatten`

## Honest status

Edge on 15d real L2 is currently **fail** (~−9 bps mean net). Live money stays closed until `edge_profile` `status=pass`.
