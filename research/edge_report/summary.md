# Edge Research Summary (R1)

- fee_bybit_rt_bps: 11.0 (Bybit entry+exit; signal venue free)
- fee_buffer_bps: 3.0
- method: event
- impulse_min_bps: 12.0
- lag_min_bps: 32.0
- max_staleness_ms: 200
- latency_ms: 150
- bar_ms: 100

## BTCUSDT coverage
- bn_rows: 14005943, by_rows: 21994840
- overlap_hours: 402.23 (bn_span_h=402.23, by_span_h=402.23)

## BTCUSDT CCF
- best_lag_ms: 100
- best_corr: 0.757

## BTCUSDT (event)
- no impulse events

## ETHUSDT coverage
- bn_rows: 13042083, by_rows: 30084322
- overlap_hours: 402.23 (bn_span_h=402.23, by_span_h=402.23)

## ETHUSDT CCF
- best_lag_ms: 0
- best_corr: 0.985

## ETHUSDT (event)
- no impulse events


> **R1 conclusion:** no monetizable net edge under Bybit RT fees + L2 slip on this window — paper/live remain closed.

**Status:** fail
