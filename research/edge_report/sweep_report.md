# R1 Parameter Sweep

Fee Bybit RT: **11.0 bps**. Notional: $3000.

## Coverage

- **BTCUSDT**: bn=13833143, by=21822040, overlap_h=360.0
- **ETHUSDT**: bn=12869283, by=29911522, overlap_h=360.0

## Pass candidates (best_hour_net>0 & ≥3 hours)

```
 symbol method  impulse_min_bps  latency_ms  bar_ms  n_events  mean_gross_bps  best_hour_gross_bps  mean_net_bps  best_hour_net_bps  follow_through  positive_hours  med_slip_bps  pass_candidate
BTCUSDT  event             12.0         150     100        18           8.221               26.528        -3.300             15.007           0.889               3         0.521            True
BTCUSDT  event             12.0         150      50        19          10.561               26.528        -0.960             15.007           0.947               5         0.521            True
BTCUSDT  event             12.0         250      50        19          10.361               22.970        -1.160             11.450           0.947               3         0.521            True
BTCUSDT  event             12.0          80      50        19           9.205               18.319        -2.316              6.798           0.947               3         0.521            True
BTCUSDT  event              8.0         150      50        76           6.250               16.665        -5.271              5.144           0.829               3         0.521            True
BTCUSDT  event              8.0         150     100        74           4.745               16.665        -6.776              5.144           0.784               3         0.521            True
BTCUSDT  event              8.0          80     100        74           4.241               16.660        -7.280              5.139           0.865               3         0.521            True
ETHUSDT  event             12.0         250      50        73           9.228               14.169        -2.293              2.648           0.822               3         0.521            True
```

## Top 15 by best_hour_net_bps (may still be negative)

```
 symbol method  impulse_min_bps  latency_ms  bar_ms  n_events  mean_gross_bps  best_hour_gross_bps  mean_net_bps  best_hour_net_bps  follow_through  positive_hours  med_slip_bps  pass_candidate
BTCUSDT  event             12.0         150      50        19          10.561               26.528        -0.960             15.007           0.947               5         0.521            True
BTCUSDT  event             12.0         150     100        18           8.221               26.528        -3.300             15.007           0.889               3         0.521            True
BTCUSDT  event             12.0         250      50        19          10.361               22.970        -1.160             11.450           0.947               3         0.521            True
BTCUSDT  event             12.0         250     100        18           8.250               22.072        -3.271             10.551           0.889               2         0.521           False
BTCUSDT    bar             12.0         150      50        19           6.878               21.254        -4.643              9.733           0.895               1         0.521           False
BTCUSDT    bar             12.0          80      50        19           7.114               19.617        -4.407              8.097           0.895               2         0.521           False
BTCUSDT    bar             12.0         250      50        19           6.587               19.617        -4.934              8.097           0.895               2         0.521           False
BTCUSDT  event             12.0          80      50        19           9.205               18.319        -2.316              6.798           0.947               3         0.521            True
BTCUSDT  event             12.0          80     100        18           6.692               18.319        -4.829              6.798           1.000               2         0.521           False
ETHUSDT  event              8.0         250      50       255           6.116               17.982        -5.405              6.461           0.780               1         0.521           False
ETHUSDT  event              8.0         150      50       255           6.076               17.569        -5.445              6.048           0.804               1         0.521           False
BTCUSDT  event              8.0         150     100        74           4.745               16.665        -6.776              5.144           0.784               3         0.521            True
BTCUSDT  event              8.0         150      50        76           6.250               16.665        -5.271              5.144           0.829               3         0.521            True
BTCUSDT  event              8.0          80     100        74           4.241               16.660        -7.280              5.139           0.865               3         0.521            True
ETHUSDT  event              8.0          80      50       255           6.008               15.553        -5.513              4.033           0.835               1         0.521           False
```

**Any pass_candidate:** True (legacy best-hour definition; several BTC event rows).

**Honest note (R1):** those candidates have **mean_net_bps ≤ 0** and often **n_events < 50**.
Production unlock now requires `mean_net > 0`, `best_hour > 0`, ≥3 hours, **and n≥50**.
Re-check: no candidate meets the honest bar → keep `status=fail`.

Production gate unchanged: do not set `mode=paper/live` unless `analyze_lead_lag.py` writes `status=pass` with real ≥14d data.

