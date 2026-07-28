# Forecast calibration (conditional EV, not price oracle)
- fee_rt_bps: 11.0
- fee_buffer_bps: 3.0
- rule: trade only if E[net] = mean(signed_capture) - fees > 0

## BTCUSDT
- n: 356
- P(follow): 0.671
- mean signed capture_bps: 2.10
- mean net after fees: -8.90
- tradeable if mean_net>0: False
- long: n=190 mean_net=-8.54 p_follow=0.721
- short: n=166 mean_net=-9.31 p_follow=0.614

## ETHUSDT
- n: 1156
- P(follow): 0.619
- mean signed capture_bps: 1.80
- mean net after fees: -9.20
- tradeable if mean_net>0: False
- long: n=624 mean_net=-9.01 p_follow=0.646
- short: n=532 mean_net=-9.42 p_follow=0.586

