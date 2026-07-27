#!/usr/bin/env python3
"""R1 parameter sweep: impulse × latency × bar_ms → heatmap CSV + markdown."""

from __future__ import annotations

import argparse
import itertools
import json
import sys
from pathlib import Path
from typing import List

import pandas as pd

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

from analyze_lead_lag import (
    DEFAULT_FEE_BPS,
    coverage_report,
    edge_with_slippage,
    impulse_events_bar,
    impulse_events_event_time,
    load_l2,
    load_trades,
    summarize_symbol,
)
from orderbook_sim import OrderBookSimulator


def run_sweep(
    trades: pd.DataFrame,
    sim: OrderBookSimulator,
    symbols: List[str],
    notional: float,
    fee_bps: float,
) -> pd.DataFrame:
    impulses = [3.0, 5.0, 8.0, 12.0]
    latencies = [80, 150, 250]
    bars = [50, 100]
    methods = ["event", "bar"]
    rows = []
    for sym, impulse, lat, bar, method in itertools.product(
        symbols, impulses, latencies, bars, methods
    ):
        if trades[trades["symbol"] == sym].empty:
            continue
        if method == "bar":
            events = impulse_events_bar(
                trades,
                sym,
                bar_ms=bar,
                impulse_min_bps=impulse,
                latency_ms=lat,
            )
        else:
            events = impulse_events_event_time(
                trades,
                sym,
                impulse_min_bps=impulse,
                latency_ms=lat,
                detect_bar_ms=bar,
            )
        events = edge_with_slippage(events, sim, sym, notional, fee_bps=fee_bps)
        cfg, hours, best_net, med_slip, best_gross = summarize_symbol(events)
        n = len(events)
        mean_ft = float(events["aligned"].mean()) if n else 0.0
        mean_gross = float(events["gross_mid_bps"].mean()) if n else 0.0
        mean_net = float(events["net_edge_bps"].mean()) if n else 0.0
        pass_cand = bool(
            cfg
            and mean_net > 0.0
            and best_net > 0.0
            and len(hours) >= 3
            and n >= 50
        )
        rows.append(
            {
                "symbol": sym,
                "method": method,
                "impulse_min_bps": impulse,
                "latency_ms": lat,
                "bar_ms": bar,
                "n_events": n,
                "mean_gross_bps": round(mean_gross, 3),
                "best_hour_gross_bps": round(best_gross, 3) if cfg else 0.0,
                "mean_net_bps": round(mean_net, 3),
                "best_hour_net_bps": round(best_net, 3) if cfg else 0.0,
                "follow_through": round(mean_ft, 3),
                "positive_hours": len(hours),
                "med_slip_bps": round(med_slip, 3) if cfg else 0.0,
                "pass_candidate": pass_cand,
            }
        )
        print(
            f"{sym} {method} impulse={impulse} lat={lat} bar={bar} "
            f"net_best={rows[-1]['best_hour_net_bps']} hours={len(hours)} n={n}",
            flush=True,
        )
    return pd.DataFrame(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description="R1 edge parameter sweep")
    parser.add_argument("--hist-dir", type=Path, default=Path("research/data/hist"))
    parser.add_argument("--symbols", nargs="+", default=["BTCUSDT", "ETHUSDT"])
    parser.add_argument("--notional-usd", type=float, default=3000.0)
    parser.add_argument("--fee-bps", type=float, default=DEFAULT_FEE_BPS)
    parser.add_argument(
        "--output-csv",
        type=Path,
        default=Path("research/edge_report/sweep_heatmap.csv"),
    )
    parser.add_argument(
        "--output-md",
        type=Path,
        default=Path("research/edge_report/sweep_report.md"),
    )
    parser.add_argument(
        "--output-json",
        type=Path,
        default=Path("research/edge_report/sweep_best.json"),
    )
    args = parser.parse_args()

    trades = load_trades(args.hist_dir)
    sim = OrderBookSimulator(load_l2(args.hist_dir))
    df = run_sweep(trades, sim, args.symbols, args.notional_usd, args.fee_bps)

    args.output_csv.parent.mkdir(parents=True, exist_ok=True)
    df.to_csv(args.output_csv, index=False)

    lines = [
        "# R1 Parameter Sweep",
        "",
        f"Fee Bybit RT: **{args.fee_bps} bps**. Notional: ${args.notional_usd:.0f}.",
        "",
        "## Coverage",
        "",
    ]
    for sym in args.symbols:
        cov = coverage_report(trades, sym)
        lines.append(
            f"- **{sym}**: bn={cov['bn_rows']}, by={cov['by_rows']}, "
            f"overlap_h={cov['overlap_hours']}"
        )

    winners = df[df["pass_candidate"]].sort_values("best_hour_net_bps", ascending=False)
    lines.extend(["", "## Pass candidates (best_hour_net>0 & ≥3 hours)", ""])
    if winners.empty:
        lines.append("None. Lead-lag not monetizable under this sweep.")
    else:
        lines.append("```")
        lines.append(winners.head(20).to_string(index=False))
        lines.append("```")

    top = df.sort_values("best_hour_net_bps", ascending=False).head(15)
    lines.extend(["", "## Top 15 by best_hour_net_bps (may still be negative)", ""])
    lines.append("```")
    lines.append(top.to_string(index=False))
    lines.append("```")

    # Best per symbol/method
    best = {}
    for sym in args.symbols:
        sub = df[df["symbol"] == sym]
        if sub.empty:
            continue
        row = sub.loc[sub["best_hour_net_bps"].idxmax()]
        best[sym] = row.to_dict()
    args.output_json.write_text(json.dumps(best, indent=2), encoding="utf-8")

    any_pass = bool(df["pass_candidate"].any())
    lines.extend(
        [
            "",
            f"**Any pass_candidate:** {any_pass}",
            "",
            "Production gate unchanged: do not set `mode=paper/live` unless "
            "`analyze_lead_lag.py` writes `status=pass` with real ≥14d data.",
            "",
        ]
    )
    args.output_md.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"CSV -> {args.output_csv}")
    print(f"MD  -> {args.output_md}")
    print(f"any_pass_candidate={any_pass}")


if __name__ == "__main__":
    main()
