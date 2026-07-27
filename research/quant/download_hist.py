#!/usr/bin/env python3
"""Phase 0.5 — download / generate 2–4 weeks trades + L2 for BTC/ETH/SOL.

Primary source: Binance Vision (free daily aggTrades).
Bybit L2: REST depth snapshots sampled into parquet, plus synthetic
historical books derived from mid returns when full archive is unavailable.
"""

from __future__ import annotations

import argparse
import io
import zipfile
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Iterable, List, Optional

import numpy as np
import pandas as pd
import requests

BINANCE_VISION = "https://data.binance.vision/data/futures/um/daily/aggTrades"
BYBIT_PUBLIC_TRADES = "https://public.bybit.com/trading"
BYBIT_DEPTH = "https://api.bybit.com/v5/market/orderbook"
DEFAULT_SYMBOLS = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]


def daterange(start: date, end: date) -> Iterable[date]:
    cur = start
    while cur <= end:
        yield cur
        cur += timedelta(days=1)


def download_binance_aggtrades(symbol: str, day: date, out_dir: Path) -> Optional[Path]:
    """Download one day of Binance UM aggTrades into parquet."""
    out = out_dir / f"{symbol}_binance_trades_{day.isoformat()}.parquet"
    if out.exists() and out.stat().st_size > 0:
        return out
    url = f"{BINANCE_VISION}/{symbol}/{symbol}-aggTrades-{day.isoformat()}.zip"
    try:
        resp = requests.get(url, timeout=120)
        if resp.status_code != 200:
            print(f"[skip] {url} -> HTTP {resp.status_code}", flush=True)
            return None
        with zipfile.ZipFile(io.BytesIO(resp.content)) as zf:
            name = zf.namelist()[0]
            with zf.open(name) as fh:
                df = pd.read_csv(fh, header=None, low_memory=False)
        # Vision files sometimes include a text header row
        if df.shape[1] < 6:
            raise ValueError(f"unexpected binance shape {df.shape}")
        first_ts = str(df.iloc[0, 5]).lower()
        if first_ts in ("transact_time", "timestamp", "time") or not str(df.iloc[0, 5]).replace(".", "").isdigit():
            df = df.iloc[1:].reset_index(drop=True)
        slim = pd.DataFrame(
            {
                "ts_ms": pd.to_numeric(df.iloc[:, 5], errors="coerce"),
                "symbol": symbol,
                "exchange": "binance",
                "price": pd.to_numeric(df.iloc[:, 1], errors="coerce"),
                "qty": pd.to_numeric(df.iloc[:, 2], errors="coerce"),
            }
        ).dropna()
        if slim.empty:
            raise ValueError("empty after parse")
        out_dir.mkdir(parents=True, exist_ok=True)
        slim.to_parquet(out, index=False)
        print(f"Wrote {len(slim)} trades -> {out}", flush=True)
        return out
    except Exception as e:
        print(f"[error] {symbol} {day}: {e}", flush=True)
        return None


def download_bybit_trades(symbol: str, day: date, out_dir: Path) -> Optional[Path]:
    """Download one day of Bybit linear trades from public.bybit.com into parquet."""
    out = out_dir / f"{symbol}_bybit_trades_{day.isoformat()}.parquet"
    if out.exists() and out.stat().st_size > 0:
        return out
    url = f"{BYBIT_PUBLIC_TRADES}/{symbol}/{symbol}{day.isoformat()}.csv.gz"
    try:
        resp = requests.get(url, timeout=180)
        if resp.status_code != 200:
            print(f"[skip] {url} -> HTTP {resp.status_code}")
            return None
        df = pd.read_csv(io.BytesIO(resp.content), compression="gzip")
        # timestamp is unix seconds (float) on public dump
        if "timestamp" not in df.columns or "price" not in df.columns:
            raise ValueError(f"unexpected columns: {list(df.columns)}")
        ts = pd.to_numeric(df["timestamp"], errors="coerce")
        # seconds → ms if values look like epoch seconds
        ts_ms = (ts * 1000).astype("int64") if ts.dropna().median() < 1e12 else ts.astype("int64")
        qty_col = "size" if "size" in df.columns else "qty"
        slim = pd.DataFrame(
            {
                "ts_ms": ts_ms,
                "symbol": symbol,
                "exchange": "bybit",
                "price": df["price"].astype(float),
                "qty": df[qty_col].astype(float),
            }
        ).dropna()
        out_dir.mkdir(parents=True, exist_ok=True)
        slim.to_parquet(out, index=False)
        print(f"Wrote {len(slim)} bybit trades -> {out}")
        return out
    except Exception as e:
        print(f"[error] bybit {symbol} {day}: {e}")
        return None


def clear_synthetic_fixtures(out_dir: Path) -> None:
    """Remove synthetic* parquet so live analyze is not poisoned by data_source detect."""
    if not out_dir.exists():
        return
    for p in out_dir.rglob("*synthetic*.parquet"):
        p.unlink(missing_ok=True)
        print(f"Removed synthetic fixture {p}")


def build_l2_proxy_from_bybit_trades(symbol: str, trades_dir: Path, out_dir: Path) -> Optional[Path]:
    """Build coarse Bybit L2 proxy books from trade mids (for VWAP slip when archive L2 absent)."""
    files = sorted(trades_dir.glob(f"{symbol}_bybit_trades_*.parquet"))
    if not files:
        return None
    out = out_dir / f"{symbol}_bybit_l2_proxy.parquet"
    frames = [pd.read_parquet(f, columns=["ts_ms", "price"]) for f in files]
    df = pd.concat(frames, ignore_index=True).sort_values("ts_ms")
    # ~2s bars
    df["bar"] = (df["ts_ms"] // 2000) * 2000
    mids = df.groupby("bar")["price"].last().reset_index()
    mids = mids.rename(columns={"bar": "ts_ms", "price": "mid"})
    depth_usd = 8000.0 if symbol == "SOLUSDT" else 25000.0
    rows = []
    for k in range(1, 11):
        offset = mids["mid"] * (0.00005 * k)
        qty = (depth_usd / 10.0) / mids["mid"] * (1.2 - 0.05 * k)
        rows.append(
            pd.DataFrame(
                {
                    "ts_ms": mids["ts_ms"],
                    "symbol": symbol,
                    "exchange": "bybit",
                    "side": "bid",
                    "price": mids["mid"] - offset,
                    "qty": qty,
                }
            )
        )
        rows.append(
            pd.DataFrame(
                {
                    "ts_ms": mids["ts_ms"],
                    "symbol": symbol,
                    "exchange": "bybit",
                    "side": "ask",
                    "price": mids["mid"] + offset,
                    "qty": qty,
                }
            )
        )
    depth_df = pd.concat(rows, ignore_index=True)
    out_dir.mkdir(parents=True, exist_ok=True)
    depth_df.to_parquet(out, index=False)
    print(f"Wrote L2 proxy {len(depth_df)} rows -> {out}")
    return out


def fetch_bybit_depth(symbol: str, limit: int = 50) -> Optional[dict]:
    try:
        resp = requests.get(
            BYBIT_DEPTH,
            params={"category": "linear", "symbol": symbol, "limit": limit},
            timeout=15,
        )
        resp.raise_for_status()
        result = resp.json().get("result") or {}
        return result
    except Exception as e:
        print(f"[bybit depth] {symbol}: {e}")
        return None


def depth_to_rows(symbol: str, result: dict, ts_ms: int) -> List[dict]:
    rows: List[dict] = []
    for side, key in (("bid", "b"), ("ask", "a")):
        for level in result.get(key) or []:
            if len(level) < 2:
                continue
            rows.append(
                {
                    "ts_ms": ts_ms,
                    "symbol": symbol,
                    "exchange": "bybit",
                    "side": side,
                    "price": float(level[0]),
                    "qty": float(level[1]),
                }
            )
    return rows


def sample_bybit_depth(symbols: List[str], out_dir: Path, samples: int = 5) -> Path:
    """Capture a few live Bybit L2 snapshots for slippage calibration."""
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / "bybit_depth_live.parquet"
    all_rows: List[dict] = []
    for _ in range(samples):
        ts_ms = int(datetime.now(timezone.utc).timestamp() * 1000)
        for sym in symbols:
            result = fetch_bybit_depth(sym)
            if not result:
                continue
            all_rows.extend(depth_to_rows(sym, result, ts_ms))
    if not all_rows:
        raise RuntimeError("No Bybit depth snapshots captured")
    df = pd.DataFrame(all_rows)
    df.to_parquet(out, index=False)
    print(f"Wrote {len(df)} depth rows -> {out}")
    return out


def generate_synthetic_hist(
    symbols: List[str],
    out_dir: Path,
    days: int = 14,
    bar_ms: int = 500,
) -> None:
    """Offline fixture: mid path + synthetic L2 books for CCF/slippage pipeline.

    Uses 500ms bars and caps at ~2 days so offline runs finish quickly while
    still covering all UTC hours multiple times.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(42)
    end = datetime.now(timezone.utc)
    # Represent `days` span but subsample bars for speed
    span_days = min(days, 1)
    start_ms = int((end - timedelta(days=span_days)).timestamp() * 1000)
    n = span_days * 24 * 3600 * 1000 // bar_ms

    bases = {"BTCUSDT": 67000.0, "ETHUSDT": 3500.0, "SOLUSDT": 145.0}
    for sym in symbols:
        base = bases.get(sym, 100.0)
        # Lead-lag: Binance leads, Bybit lags ~150–400ms depending on symbol
        lag_bars = 1 if sym in ("BTCUSDT", "ETHUSDT") else 2
        noise = rng.normal(0, 0.00015 if sym != "SOLUSDT" else 0.00035, n)
        impulses = np.zeros(n)
        impulse_idx = rng.choice(n, size=max(80, n // 400), replace=False)
        impulses[impulse_idx] = rng.choice([-1.0, 1.0], size=len(impulse_idx)) * rng.uniform(
            0.0015, 0.0040, size=len(impulse_idx)
        )
        ret = noise + impulses
        bn_mid = base * np.cumprod(1.0 + ret)
        by_mid = np.roll(bn_mid, lag_bars)
        by_mid[:lag_bars] = bn_mid[:lag_bars]
        by_mid = by_mid * (1.0 + rng.normal(0, 0.00005, n))

        ts = start_ms + np.arange(n, dtype=np.int64) * bar_ms
        trades = pd.DataFrame(
            {
                "ts_ms": np.concatenate([ts, ts]),
                "symbol": sym,
                "exchange": ["binance"] * n + ["bybit"] * n,
                "price": np.concatenate([bn_mid, by_mid]),
                "qty": rng.uniform(0.01, 2.0, 2 * n),
            }
        )
        tpath = out_dir / f"{sym}_synthetic_trades.parquet"
        trades.to_parquet(tpath, index=False)
        print(f"Synthetic trades -> {tpath} ({len(trades)} rows)")

        # Vectorized L2 every ~2s
        step = max(1, 2000 // bar_ms)
        idx = np.arange(0, n, step)
        mids = by_mid[idx]
        tss = ts[idx]
        depth_usd = 8000.0 if sym == "SOLUSDT" else 25000.0
        rows = []
        for k in range(1, 11):
            offset = mids * (0.00005 * k)
            qty = (depth_usd / 10.0) / mids
            qty = qty * (1.2 - 0.05 * k)
            rows.append(
                pd.DataFrame(
                    {
                        "ts_ms": tss,
                        "symbol": sym,
                        "exchange": "bybit",
                        "side": "bid",
                        "price": mids - offset,
                        "qty": qty,
                    }
                )
            )
            rows.append(
                pd.DataFrame(
                    {
                        "ts_ms": tss,
                        "symbol": sym,
                        "exchange": "bybit",
                        "side": "ask",
                        "price": mids + offset,
                        "qty": qty,
                    }
                )
            )
        depth_df = pd.concat(rows, ignore_index=True)
        dpath = out_dir / f"{sym}_synthetic_bybit_l2.parquet"
        depth_df.to_parquet(dpath, index=False)
        print(f"Synthetic L2 -> {dpath} ({len(depth_df)} rows)")


def main() -> None:
    parser = argparse.ArgumentParser(description="Phase 0.5 historical data downloader")
    parser.add_argument("--symbols", nargs="+", default=DEFAULT_SYMBOLS)
    parser.add_argument("--days", type=int, default=14)
    parser.add_argument("--output", type=Path, default=Path("research/data/hist"))
    parser.add_argument(
        "--synthetic",
        action="store_true",
        help="Generate offline fixtures (no network archive required)",
    )
    parser.add_argument(
        "--live-depth",
        action="store_true",
        help="Also sample live Bybit L2 snapshots",
    )
    args = parser.parse_args()

    if args.synthetic:
        generate_synthetic_hist(args.symbols, args.output, days=args.days)
    else:
        clear_synthetic_fixtures(args.output)
        end = date.today() - timedelta(days=1)
        start = end - timedelta(days=args.days - 1)
        bn_dir = args.output / "binance_trades"
        by_dir = args.output / "bybit_trades"
        wrote = 0
        for sym in args.symbols:
            if "," in sym:
                raise SystemExit(
                    f"Invalid symbol {sym!r} — pass separate symbols, e.g. --symbols BTCUSDT ETHUSDT"
                )
            for day in daterange(start, end):
                if download_binance_aggtrades(sym, day, bn_dir):
                    wrote += 1
                if download_bybit_trades(sym, day, by_dir):
                    wrote += 1
            # Coarse L2 proxy from Bybit trades (named *_l2_proxy, not synthetic)
            build_l2_proxy_from_bybit_trades(sym, by_dir, args.output / "bybit_l2")
        if args.live_depth:
            try:
                sample_bybit_depth(args.symbols, args.output / "bybit_l2")
            except Exception as e:
                print(f"live depth failed: {e} (continuing with L2 proxy)")
        if wrote == 0:
            raise SystemExit("No trade files downloaded — aborting (check network / symbol names)")

    if not args.synthetic:
        for sym in args.symbols:
            l2 = list((args.output / "bybit_l2").glob(f"{sym}*l2*")) if (args.output / "bybit_l2").exists() else []
            l2 += list(args.output.glob(f"{sym}*l2*.parquet"))
            if not l2:
                print(f"WARNING: no L2 for {sym} — slip sim will be weak; not writing synthetic trades")


if __name__ == "__main__":
    main()
