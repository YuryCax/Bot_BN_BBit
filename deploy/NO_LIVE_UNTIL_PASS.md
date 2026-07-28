# Gate: no paper / live / paid dual-node until edge pass

Hard rule (ТЗ v2.4 / ADR-003):

1. `config/edge_profile.toml` must have `status = "pass"`, `data_source` ∈ {`live`,`binance_vision`}, `research_period_days ≥ 14`, `research_method = "l2_vwap"`.
2. Keep `[deployment] mode = "dev"` until then (`config.testnet.toml` is also `dev` by default).
3. Do **not** rent Tokyo+Singapore for trading while fail — AWS only for optional latency wiring after pass, or explicit non-trading smoke.
4. Local wiring smoke: `.\scripts\smoke_mono_node.ps1` (`mode=dev` only).
5. Dual-node wiring smoke (no orders): [`scripts/smoke_dual_node_check.md`](../scripts/smoke_dual_node_check.md) + Zenoh examples under `deploy/`.
6. Secrets: `/etc/bot/secrets.env` from `deploy/secrets.env.example` — never commit keys.

If R1 sweep finds a candidate, re-run `analyze_lead_lag.py` with those params and only then consider paper.
