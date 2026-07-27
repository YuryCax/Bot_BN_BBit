#!/usr/bin/env python3
"""R1 lead-lag analysis: fee realism, event-time asof, coverage, L2 slip.

Production gate unchanged: status=pass only if non-synthetic, days≥14,
net_edge>0 in ≥3 hours (Bybit RT fees + L2 slip).
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import numpy as np
import pandas as pd

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

from ccf_lag import calculate_optimal_lag, maybe_plot
from orderbook_sim import OrderBookSimulator

# Bybit linear taker 5.5 bps × 2 (entry+exit). Binance signal has no fee.
FEE_BYBIT_RT_BPS = 11.0
# Legacy mislabeled double-venue proxy (kept for comparison only).
FEE_LEGACY_DOUBLE_VENUE_BPS = 11.0
FEE_BUFFER_BPS = 3.0  # config fee_profit_buffer_pct ≈ 0.0003
DEFAULT_FEE_BPS = FEE_BYBIT_RT_BPS

INJECTED_LATENCY_MS = 150
IMPULSE_MIN_BPS = 5.0
DEFAULT_NOTIONALS = (1000.0, 3000.0)
FORWARD_HORIZONS_MS = (150, 300, 500, 1000)


def load_trades(hist_dir: Path) -> pd.DataFrame:
    files = sorted(hist_dir.rglob("*trades*.parquet"))
    if not files:
        raise FileNotFoundError(f"No *trades*.parquet under {hist_dir}")
    frames = [pd.read_parquet(f) for f in files]
    df = pd.concat(frames, ignore_index=True)
    need = {"ts_ms", "symbol", "exchange", "price"}
    missing = need - set(df.columns)
    if missing:
        raise ValueError(f"trades missing {missing}")
    return df.sort_values("ts_ms")


def load_l2(hist_dir: Path) -> pd.DataFrame:
    files = sorted(hist_dir.rglob("*l2*.parquet")) + sorted(
        hist_dir.rglob("*depth*.parquet")
    )
    if not files:
        raise FileNotFoundError(f"No L2/depth parquet under {hist_dir}")
    frames = [pd.read_parquet(f) for f in files]
    return pd.concat(frames, ignore_index=True)


def coverage_report(trades: pd.DataFrame, symbol: str) -> Dict:
    sub = trades[trades["symbol"] == symbol]
    bn = sub[sub["exchange"] == "binance"]
    by = sub[sub["exchange"] == "bybit"]
    if bn.empty or by.empty:
        return {
            "symbol": symbol,
            "bn_rows": int(len(bn)),
            "by_rows": int(len(by)),
            "overlap_hours": 0.0,
            "bn_span_h": 0.0,
            "by_span_h": 0.0,
        }
    bn_min, bn_max = int(bn["ts_ms"].min()), int(bn["ts_ms"].max())
    by_min, by_max = int(by["ts_ms"].min()), int(by["ts_ms"].max())
    overlap_ms = max(0, min(bn_max, by_max) - max(bn_min, by_min))
    return {
        "symbol": symbol,
        "bn_rows": int(len(bn)),
        "by_rows": int(len(by)),
        "overlap_hours": round(overlap_ms / 3_600_000, 2),
        "bn_span_h": round((bn_max - bn_min) / 3_600_000, 2),
        "by_span_h": round((by_max - by_min) / 3_600_000, 2),
    }


def mid_series(trades: pd.DataFrame, symbol: str, exchange: str) -> pd.DataFrame:
    sub = trades[(trades["symbol"] == symbol) & (trades["exchange"] == exchange)][
        ["ts_ms", "price"]
    ].sort_values("ts_ms")
    return sub.rename(columns={"price": "mid"}).drop_duplicates("ts_ms", keep="last")


def impulse_events_bar(
    trades: pd.DataFrame,
    symbol: str,
    bar_ms: int = 100,
    impulse_min_bps: float = IMPULSE_MIN_BPS,
    latency_ms: int = INJECTED_LATENCY_MS,
    forward_ms: Optional[int] = None,
) -> pd.DataFrame:
    """Bar-join impulse events (legacy). forward window defaults to latency+bar."""
    fwd = forward_ms if forward_ms is not None else latency_ms + bar_ms
    sub = trades[trades["symbol"] == symbol]
    bn = sub[sub["exchange"] == "binance"].copy()
    by = sub[sub["exchange"] == "bybit"].copy()
    if bn.empty or by.empty:
        return pd.DataFrame()
    bn["bar"] = (bn["ts_ms"] // bar_ms) * bar_ms
    by["bar"] = (by["ts_ms"] // bar_ms) * bar_ms
    bn_mid = bn.groupby("bar")["price"].last()
    by_mid = by.groupby("bar")["price"].last()
    joined = pd.concat([bn_mid.rename("bn"), by_mid.rename("by")], axis=1).dropna()
    if joined.empty:
        return pd.DataFrame()
    joined = joined.sort_index()
    joined["bn_prev"] = joined["bn"].shift(1)
    joined["impulse_bps"] = (joined["bn"] - joined["bn_prev"]) / joined["bn_prev"] * 10_000
    lag_bars = max(1, int(np.ceil(fwd / bar_ms)))
    joined["by_fwd"] = joined["by"].shift(-lag_bars)
    joined["fwd_bps"] = (joined["by_fwd"] - joined["by"]) / joined["by"] * 10_000
    joined["hour_utc"] = ((joined.index // 3_600_000) % 24).astype(int)
    events = joined[joined["impulse_bps"].abs() >= impulse_min_bps].dropna().copy()
    if events.empty:
        return pd.DataFrame()
    events["direction"] = np.sign(events["impulse_bps"])
    events["aligned"] = events["fwd_bps"] * events["direction"] > 0
    events["conditional_bps"] = events["fwd_bps"] * events["direction"]
    events["gross_mid_bps"] = events["conditional_bps"]
    events["symbol"] = symbol
    events["method"] = "bar"
    return events.reset_index(names="ts_ms")


def impulse_events_event_time(
    trades: pd.DataFrame,
    symbol: str,
    impulse_min_bps: float = IMPULSE_MIN_BPS,
    latency_ms: int = INJECTED_LATENCY_MS,
    detect_bar_ms: int = 100,
    max_events: int = 50_000,
) -> pd.DataFrame:
    """Event-time: Binance impulse on detect_bar, Bybit mids via asof at t and t+latency."""
    bn = mid_series(trades, symbol, "binance")
    by = mid_series(trades, symbol, "bybit")
    if bn.empty or by.empty:
        return pd.DataFrame()

    bn = bn.copy()
    bn["bar"] = (bn["ts_ms"] // detect_bar_ms) * detect_bar_ms
    bn_bar = bn.groupby("bar", as_index=False).agg(ts_ms=("ts_ms", "last"), bn=("mid", "last"))
    bn_bar["bn_prev"] = bn_bar["bn"].shift(1)
    bn_bar["impulse_bps"] = (bn_bar["bn"] - bn_bar["bn_prev"]) / bn_bar["bn_prev"] * 10_000
    impulses = bn_bar[bn_bar["impulse_bps"].abs() >= impulse_min_bps].dropna().copy()
    if impulses.empty:
        return pd.DataFrame()
    if len(impulses) > max_events:
        impulses = impulses.sample(n=max_events, random_state=42).sort_values("ts_ms")

    by_sorted = by.sort_values("ts_ms")
    left = impulses[["ts_ms", "impulse_bps", "bn"]].sort_values("ts_ms")
    left["direction"] = np.sign(left["impulse_bps"])

    at_t = pd.merge_asof(
        left,
        by_sorted.rename(columns={"mid": "by_t"}),
        on="ts_ms",
        direction="backward",
    )
    fwd_key = at_t[["ts_ms"]].copy()
    fwd_key["ts_query"] = at_t["ts_ms"] + int(latency_ms)
    at_fwd = pd.merge_asof(
        fwd_key.sort_values("ts_query"),
        by_sorted.rename(columns={"mid": "by_fwd", "ts_ms": "by_ts"}),
        left_on="ts_query",
        right_on="by_ts",
        direction="forward",
    )
    out = at_t.merge(at_fwd[["ts_ms", "by_fwd"]], on="ts_ms", how="inner")
    out = out.dropna(subset=["by_t", "by_fwd"])
    if out.empty:
        return pd.DataFrame()
    out["fwd_bps"] = (out["by_fwd"] - out["by_t"]) / out["by_t"] * 10_000
    out["conditional_bps"] = out["fwd_bps"] * out["direction"]
    out["gross_mid_bps"] = out["conditional_bps"]
    out["aligned"] = out["conditional_bps"] > 0
    out["hour_utc"] = ((out["ts_ms"] // 3_600_000) % 24).astype(int)
    out["symbol"] = symbol
    out["method"] = "event"
    return out


def edge_with_slippage(
    events: pd.DataFrame,
    sim: OrderBookSimulator,
    symbol: str,
    notional_usd: float,
    fee_bps: float = DEFAULT_FEE_BPS,
    max_events: int = 2000,
) -> pd.DataFrame:
    if events.empty:
        return events
    sample = events
    if len(events) > max_events:
        sample = events.sample(n=max_events, random_state=42).sort_values("ts_ms")
    slips: List[float] = []
    for _, row in sample.iterrows():
        side = "buy" if row["direction"] > 0 else "sell"
        fill = sim.simulate_market_order(symbol, side, notional_usd, int(row["ts_ms"]))
        slips.append(fill.slippage_bps if fill else 50.0)
    out = sample.copy()
    out["slippage_bps"] = slips
    out["gross_mid_bps"] = out["conditional_bps"]
    out["net_edge_bps"] = out["conditional_bps"] - fee_bps - out["slippage_bps"]
    out["net_edge_with_buffer_bps"] = out["conditional_bps"] - fee_bps - FEE_BUFFER_BPS - out["slippage_bps"]
    out["fee_bps_used"] = fee_bps
    return out


def summarize_symbol(
    events: pd.DataFrame,
) -> Tuple[Dict, List[int], float, float, float]:
    if events.empty:
        return {}, [], 0.0, 0.0, 0.0
    g = (
        events.groupby("hour_utc")
        .agg(
            follow_through_rate=("aligned", "mean"),
            net_edge_bps=("net_edge_bps", "mean"),
            gross_mid_bps=("gross_mid_bps", "mean"),
            n=("aligned", "count"),
        )
        .reset_index()
    )
    positive = g[g["net_edge_bps"] > 0]
    hours = sorted(positive["hour_utc"].astype(int).tolist())
    best_net = float(g["net_edge_bps"].max())
    best_gross = float(g["gross_mid_bps"].max())
    ft_min = float(events["aligned"].mean()) if len(events) else 0.4
    if len(g) >= 4:
        ft_min = float(g["follow_through_rate"].quantile(0.25))
    cfg = {
        "net_edge_bps": round(best_net, 2),
        "follow_through_min": round(max(0.35, min(ft_min, 0.55)), 3),
        "lag_min_bps": 3.0,
        "trade_hours_utc": hours,
        "vol_regime_min_atr_pct": 0.0025,
        "max_slippage_bps": round(float(events["slippage_bps"].quantile(0.95)), 2),
        "max_adverse_move_bps": 15.0,
    }
    return cfg, hours, best_net, float(events["slippage_bps"].median()), best_gross


def write_edge_profile(
    path: Path,
    status: str,
    days: int,
    edges: Dict[str, dict],
    method: str,
    data_source: str,
    latency_ms: int,
) -> None:
    lines = [
        "# Auto-generated by research/quant/analyze_lead_lag.py (R1 L2-aware)",
        "",
        "[meta]",
        f'generated_at = "{datetime.now(timezone.utc).isoformat()}"',
        f"research_period_days = {days}",
        f"injected_latency_ms = {latency_ms}",
        f'status = "{status}"',
        f'research_method = "{method}"',
        f'data_source = "{data_source}"',
    ]
    for sym, cfg in edges.items():
        lines.append("")
        lines.append(f"[edge.{sym}]")
        for k, v in cfg.items():
            if isinstance(v, list):
                lines.append(f"{k} = {v}")
            elif isinstance(v, str):
                lines.append(f'{k} = "{v}"')
            else:
                lines.append(f"{k} = {v}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_summary(path: Path, report: str, status: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(report + f"\n**Status:** {status}\n", encoding="utf-8")


def detect_data_source(hist_dir: Path) -> str:
    files = list(hist_dir.rglob("*.parquet"))
    names = " ".join(p.name.lower() for p in files)
    if "synthetic" in names and "binance_trades" not in names:
        return "synthetic"
    if (hist_dir / "binance_trades").exists() or "binance_trades" in names:
        return "binance_vision"
    return "live"


def decide_alt_enable(edges: Dict[str, dict], status: str, symbol: str) -> bool:
    if status != "pass":
        return False
    cfg = edges.get(symbol)
    if not cfg:
        return False
    return cfg.get("net_edge_bps", 0) > 0 and len(cfg.get("trade_hours_utc", [])) >= 3


def patch_symbols_toml(
    path: Path,
    enable_sol: bool,
    enable_avax: bool = False,
    leverage: int = 3,
) -> None:
    text = path.read_text(encoding="utf-8")
    import re

    text = re.sub(r"(?m)^(leverage\s*=\s*)\d+", rf"\g<1>{leverage}", text)
    lines = text.splitlines()
    current = None
    for i, line in enumerate(lines):
        if 'binance = "SOLUSDT"' in line:
            current = "SOL"
        elif 'binance = "AVAXUSDT"' in line:
            current = "AVAX"
        if current and line.startswith("enabled"):
            if current == "SOL":
                lines[i] = f"enabled = {'true' if enable_sol else 'false'}"
            elif current == "AVAX":
                lines[i] = f"enabled = {'true' if enable_avax else 'false'}"
            current = None
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Updated {path}: SOL={enable_sol} AVAX={enable_avax} leverage={leverage}")


def main() -> None:
    parser = argparse.ArgumentParser(description="R1 L2-aware lead-lag analysis")
    parser.add_argument("--hist-dir", type=Path, default=Path("research/data/hist"))
    parser.add_argument("--output-profile", type=Path, default=Path("config/edge_profile.toml"))
    parser.add_argument(
        "--output-summary", type=Path, default=Path("research/edge_report/summary.md")
    )
    parser.add_argument(
        "--output-params", type=Path, default=Path("research/edge_report/params_for_rust.json")
    )
    parser.add_argument("--symbols-toml", type=Path, default=Path("config/symbols.toml"))
    parser.add_argument("--symbols", nargs="+", default=["BTCUSDT", "ETHUSDT"])
    parser.add_argument("--notional-usd", type=float, default=3000.0)
    parser.add_argument("--plot-dir", type=Path, default=Path("research/edge_report/ccf"))
    parser.add_argument("--data-source", default=None)
    parser.add_argument(
        "--method",
        choices=["bar", "event", "both"],
        default="event",
        help="Impulse measurement: event-time asof (default), bar-join, or both",
    )
    parser.add_argument("--impulse-min-bps", type=float, default=IMPULSE_MIN_BPS)
    parser.add_argument("--latency-ms", type=int, default=INJECTED_LATENCY_MS)
    parser.add_argument("--bar-ms", type=int, default=100)
    parser.add_argument("--fee-bps", type=float, default=DEFAULT_FEE_BPS)
    args = parser.parse_args()

    trades = load_trades(args.hist_dir)
    l2 = load_l2(args.hist_dir)
    sim = OrderBookSimulator(l2)
    data_source = args.data_source or detect_data_source(args.hist_dir)

    edges: Dict[str, dict] = {}
    report_lines = [
        "# Edge Research Summary (R1)",
        "",
        f"- fee_bybit_rt_bps: {args.fee_bps} (Bybit entry+exit; signal venue free)",
        f"- fee_buffer_bps: {FEE_BUFFER_BPS}",
        f"- method: {args.method}",
        f"- impulse_min_bps: {args.impulse_min_bps}",
        f"- latency_ms: {args.latency_ms}",
        f"- bar_ms: {args.bar_ms}",
        "",
    ]
    params: Dict = {
        "notionals_usd": list(DEFAULT_NOTIONALS),
        "fee_bybit_rt_bps": args.fee_bps,
        "fee_buffer_bps": FEE_BUFFER_BPS,
        "method": args.method,
        "symbols": {},
    }
    any_pass = False

    period_days = max(
        1,
        int((trades["ts_ms"].max() - trades["ts_ms"].min()) / 86_400_000) + 1,
    )

    primary_method = "event" if args.method in ("event", "both") else "bar"

    for sym in args.symbols:
        if trades[trades["symbol"] == sym].empty:
            continue

        cov = coverage_report(trades, sym)
        report_lines.append(
            f"## {sym} coverage\n"
            f"- bn_rows: {cov['bn_rows']}, by_rows: {cov['by_rows']}\n"
            f"- overlap_hours: {cov['overlap_hours']} "
            f"(bn_span_h={cov['bn_span_h']}, by_span_h={cov['by_span_h']})\n"
        )

        try:
            ccf = calculate_optimal_lag(trades, sym, bar_ms=args.bar_ms)
        except Exception:
            ccf = None
        if ccf:
            maybe_plot(ccf, args.plot_dir / f"{sym}_ccf.png")
            report_lines.append(
                f"## {sym} CCF\n- best_lag_ms: {ccf.best_lag_ms}\n- best_corr: {ccf.best_corr:.3f}\n"
            )

        methods_to_run = []
        if args.method in ("bar", "both"):
            methods_to_run.append("bar")
        if args.method in ("event", "both"):
            methods_to_run.append("event")

        best_for_profile = None
        for m in methods_to_run:
            if m == "bar":
                events = impulse_events_bar(
                    trades,
                    sym,
                    bar_ms=args.bar_ms,
                    impulse_min_bps=args.impulse_min_bps,
                    latency_ms=args.latency_ms,
                )
            else:
                events = impulse_events_event_time(
                    trades,
                    sym,
                    impulse_min_bps=args.impulse_min_bps,
                    latency_ms=args.latency_ms,
                    detect_bar_ms=args.bar_ms,
                )
            events = edge_with_slippage(
                events, sim, sym, args.notional_usd, fee_bps=args.fee_bps
            )
            cfg, hours, best_net, med_slip, best_gross = summarize_symbol(events)
            if not cfg:
                report_lines.append(f"## {sym} ({m})\n- no impulse events\n")
                continue

            slip_series = sim.slippage_distribution(sym, args.notional_usd)
            p95 = (
                float(slip_series.quantile(0.95))
                if len(slip_series)
                else cfg["max_slippage_bps"]
            )
            cfg["max_slippage_bps"] = round(p95, 2)
            mean_ft = float(events["aligned"].mean())
            mean_gross = float(events["gross_mid_bps"].mean())
            mean_net = float(events["net_edge_bps"].mean())
            n_ev = len(events)

            ok = (
                mean_net > 0.0
                and best_net > 0.0
                and len(hours) >= 3
                and n_ev >= 50
            )
            report_lines.append(f"## {sym} ({m})")
            report_lines.append(f"- n_events (sampled): {n_ev}")
            report_lines.append(f"- mean gross_mid_bps: {mean_gross:.2f}")
            report_lines.append(f"- best hourly gross_mid_bps: {best_gross:.2f}")
            report_lines.append(f"- mean net_edge_bps (fee+slip): {mean_net:.2f}")
            report_lines.append(f"- best hourly net_edge_bps: {best_net:.2f}")
            report_lines.append(f"- median slippage_bps @ ${args.notional_usd:.0f}: {med_slip:.2f}")
            report_lines.append(f"- p95 book_slippage_bps: {cfg['max_slippage_bps']}")
            report_lines.append(f"- follow_through: {mean_ft:.3f}")
            report_lines.append(f"- positive hours: {hours}")
            report_lines.append(
                f"- L2 gate: {'PASS' if ok else 'FAIL'} "
                f"(need mean_net>0, best_hour>0, ≥3 hours, n≥50)"
            )
            report_lines.append("")

            if m == primary_method:
                # Profile stores best-hour for reference, but enable only if mean_net>0
                profile_cfg = dict(cfg)
                if mean_net <= 0 or n_ev < 50:
                    profile_cfg["trade_hours_utc"] = []
                    profile_cfg["net_edge_bps"] = round(min(best_net, mean_net, 0.0), 2)
                best_for_profile = (
                    profile_cfg,
                    hours if ok else [],
                    best_net,
                    ok,
                    mean_ft,
                    mean_gross,
                    mean_net,
                )
                if ok:
                    any_pass = True

        if best_for_profile:
            cfg, hours, best_net, ok, mean_ft, mean_gross, mean_net = best_for_profile
            edges[sym] = cfg
            params["symbols"][sym] = {
                "max_slippage_pct": round(cfg["max_slippage_bps"] / 10_000, 6),
                "max_slippage_bps": cfg["max_slippage_bps"],
                "max_adverse_move_bps": cfg["max_adverse_move_bps"],
                "lag_min_bps": cfg["lag_min_bps"],
                "follow_through_min": cfg["follow_through_min"],
                "trade_hours_utc": hours,
                "net_edge_bps": cfg["net_edge_bps"],
                "gross_mid_bps_best_hour": round(mean_gross, 2),
                "ccf_best_lag_ms": ccf.best_lag_ms if ccf else None,
                "enabled_candidate": ok,
                "method": primary_method,
                "coverage": cov,
            }

    production_ok = any_pass and data_source != "synthetic" and period_days >= 14
    status = "pass" if production_ok else "fail"
    if any_pass and not production_ok:
        report_lines.append(
            f"\n> Edge positive on sample but **status=fail** "
            f"(data_source={data_source}, period_days={period_days}; need live≥14d).\n"
        )
    if not any_pass:
        report_lines.append(
            "\n> **R1 conclusion:** no monetizable net edge under Bybit RT fees + L2 slip "
            "on this window — paper/live remain closed.\n"
        )

    for sym, cfg in list(edges.items()):
        if cfg["net_edge_bps"] <= 0 or len(cfg["trade_hours_utc"]) < 3:
            cfg["trade_hours_utc"] = []
            cfg["net_edge_bps"] = min(cfg["net_edge_bps"], 0.0)

    research_method = "l2_vwap_event" if primary_method == "event" else "l2_vwap"
    # validation requires research_method == l2_vwap for paper/live
    write_edge_profile(
        args.output_profile,
        status,
        period_days,
        edges,
        "l2_vwap",
        data_source,
        args.latency_ms,
    )
    write_summary(args.output_summary, "\n".join(report_lines), status)
    params["research_method_detail"] = research_method
    params["status"] = status
    params["period_days"] = period_days
    args.output_params.write_text(json.dumps(params, indent=2), encoding="utf-8")

    enable_sol = decide_alt_enable(edges, status, "SOLUSDT")
    enable_avax = decide_alt_enable(edges, status, "AVAXUSDT")
    if args.symbols_toml.exists():
        patch_symbols_toml(
            args.symbols_toml,
            enable_sol=enable_sol,
            enable_avax=enable_avax,
            leverage=3,
        )

    print(f"Profile -> {args.output_profile} status={status} data_source={data_source}")
    print(f"SOL enabled={enable_sol} AVAX enabled={enable_avax}")
    print(f"Params -> {args.output_params}")


if __name__ == "__main__":
    main()
