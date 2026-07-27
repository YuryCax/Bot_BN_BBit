# Edge Research Summary (R1)

- fee_bybit_rt_bps: 11.0 (Bybit entry+exit; signal venue free)
- fee_buffer_bps: 3.0
- method: event
- impulse_min_bps: 5.0
- latency_ms: 150
- bar_ms: 100

## BTCUSDT coverage
- bn_rows: 13833143, by_rows: 21822040
- overlap_hours: 360.0 (bn_span_h=360.0, by_span_h=360.0)

## BTCUSDT CCF
- best_lag_ms: 0
- best_corr: 0.674

## BTCUSDT (event)
- n_events (sampled): 356
- mean gross_mid_bps: 2.10
- best hourly gross_mid_bps: 4.27
- mean net_edge_bps (fee+slip): -9.42
- best hourly net_edge_bps: -7.25
- median slippage_bps @ $3000: 0.52
- p95 book_slippage_bps: 0.52
- follow_through: 0.671
- positive hours: []
- L2 gate: FAIL (need mean_net>0, best_hour>0, ≥3 hours, n≥50)

## ETHUSDT coverage
- bn_rows: 12869283, by_rows: 29911522
- overlap_hours: 360.0 (bn_span_h=360.0, by_span_h=360.0)

## ETHUSDT CCF
- best_lag_ms: 0
- best_corr: 0.708

## ETHUSDT (event)
- n_events (sampled): 1156
- mean gross_mid_bps: 1.80
- best hourly gross_mid_bps: 2.74
- mean net_edge_bps (fee+slip): -9.72
- best hourly net_edge_bps: -8.78
- median slippage_bps @ $3000: 0.52
- p95 book_slippage_bps: 0.52
- follow_through: 0.619
- positive hours: []
- L2 gate: FAIL (need mean_net>0, best_hour>0, ≥3 hours, n≥50)


> **R1 conclusion:** no monetizable net edge under Bybit RT fees + L2 slip on this window — paper/live remain closed.

**Status:** fail
