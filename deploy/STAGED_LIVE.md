# Staged live checklist (ТЗ v2.4 / Sprint D) — do not enable until gates pass.



## Hard gates (all required)



1. `.\scripts\run_quant_hardening.ps1 -LiveDownload`

   - `config/edge_profile.toml`: `status=pass`, `research_period_days ≥ 14`, `data_source` ∈ {`live`,`binance_vision`}, `research_method=l2_vwap`

2. Dual-node paper ≥100 trades; PF ≥ 1.2; FT ≥ 40%; DD < 10% on **real** `logs/packets.bin` (replay without `--allow-fixture`)

3. Kill switch verified: Panel halt **and** Telegram emergency command

4. SafeMode heartbeat miss → halt entries observed in dry drill



## Live config (one pair only)



```toml

[deployment]

mode = "live"



[capital]

risk_per_trade_pct = 0.01   # 1%



[execution]

default_leverage_futures = 3

max_leverage_futures = 5

max_adverse_move_bps = 15.0

```



In `symbols.toml`: enable **one** symbol that passed L2 (usually BTC); keep SOL/AVAX off.



## Monitor (first 7 days)



- `logs/paper_ledger.jsonl` / `logs/paper_summary.txt`

- SafeMode phase on heartbeat miss

- No synthetic edge reload



## Stop conditions



- Daily DD ≥ 1.5% futures → halt

- PF collapse / FT below edge_profile → revert to paper

