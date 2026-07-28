# Testnet / session analytics
- generated_at: 2026-07-27T18:59:51.928222+00:00
- ledger: logs/fixtures/sample_testnet_ledger.jsonl
- n_closed_trades: 3

- packets: (none)

## Summary
- win_rate: 0.333
- mean_net_usdt: -0.000644
- mean_capture_bps (exit vs entry): 18.20
- total_net_usdt: -0.001932
- total_fees_usdt: 0.022182
- dry_run_fills: 0, live_api_fills: 3

## Long vs Short
- long: n=2 mean_net_usdt=0.000028 mean_capture_bps=22.30
- short: n=1 mean_net_usdt=-0.001989 mean_capture_bps=10.00

## Exit reasons (SL/TP/trail/convergence)
- TakeProfit: 1
- StopLoss: 1
- PartialTakeProfit: 1

## Hours UTC with mean net > 0
- hours: [1]

## What to change before mainnet
- capture vs fees looks viable on this sample — confirm n≥50 and edge_profile pass.
- win_rate low → review invalidation / time_stop / MICRO filters.
- candidate trade_hours_utc = [1]
- Do NOT set BOT_ALLOW_MAINNET until edge_profile.status=pass and this report mean_net>0.
