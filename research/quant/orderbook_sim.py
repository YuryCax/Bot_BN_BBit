#!/usr/bin/env python3
"""Order book VWAP slippage simulator for Phase 0.5."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, List, Optional, Tuple

import numpy as np
import pandas as pd


@dataclass
class FillResult:
    side: str
    volume_usd: float
    vwap: float
    mid: float
    slippage_bps: float
    filled_usd: float
    exhausted: bool


class OrderBookSimulator:
    """Walk historical Bybit L2 levels to estimate market-order VWAP.

    Stores per-symbol sorted snapshot timestamps and level arrays for O(log n) lookup.
    """

    def __init__(self, depth_df: pd.DataFrame):
        if depth_df.empty:
            raise ValueError("empty depth dataframe")
        need = {"ts_ms", "symbol", "side", "price", "qty"}
        missing = need - set(depth_df.columns)
        if missing:
            raise ValueError(f"depth missing columns: {missing}")

        self._books: Dict[str, dict] = {}
        for sym, g in depth_df.groupby("symbol", sort=False):
            ts_vals = np.sort(g["ts_ms"].unique())
            if len(ts_vals) > 10_000:
                idx = np.linspace(0, len(ts_vals) - 1, 10_000).astype(int)
                ts_vals = ts_vals[idx]
            ts_set = set(ts_vals.tolist())
            sub = g[g["ts_ms"].isin(ts_set)]
            snaps: Dict[int, Tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]] = {}
            for ts, sg in sub.groupby("ts_ms", sort=False):
                bids = sg[sg["side"] == "bid"].sort_values("price", ascending=False)
                asks = sg[sg["side"] == "ask"].sort_values("price", ascending=True)
                snaps[int(ts)] = (
                    bids["price"].to_numpy(dtype=float),
                    bids["qty"].to_numpy(dtype=float),
                    asks["price"].to_numpy(dtype=float),
                    asks["qty"].to_numpy(dtype=float),
                )
            self._books[str(sym)] = {
                "ts": np.array(sorted(snaps.keys()), dtype=np.int64),
                "snaps": snaps,
            }

    def nearest_book(
        self, symbol: str, ts_ms: int
    ) -> Optional[Tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray]]:
        book = self._books.get(symbol)
        if not book or len(book["ts"]) == 0:
            return None
        ts_arr: np.ndarray = book["ts"]
        i = int(np.searchsorted(ts_arr, ts_ms, side="left"))
        candidates = []
        if i < len(ts_arr):
            candidates.append(ts_arr[i])
        if i > 0:
            candidates.append(ts_arr[i - 1])
        nearest = min(candidates, key=lambda t: abs(int(t) - ts_ms))
        if abs(int(nearest) - ts_ms) > 120_000:
            return None
        return book["snaps"][int(nearest)]

    def simulate_market_order(
        self,
        symbol: str,
        side: str,
        volume_usd: float,
        ts_ms: int,
    ) -> Optional[FillResult]:
        snap = self.nearest_book(symbol, ts_ms)
        if snap is None or volume_usd <= 0:
            return None
        bid_px, bid_qty, ask_px, ask_qty = snap
        if len(bid_px) == 0 or len(ask_px) == 0:
            return None
        mid = float((bid_px[0] + ask_px[0]) / 2.0)
        if mid <= 0:
            return None

        side_l = side.lower()
        if side_l in ("buy", "long", "ask"):
            prices, qtys = ask_px, ask_qty
            trade_side = "buy"
        else:
            prices, qtys = bid_px, bid_qty
            trade_side = "sell"

        remaining = volume_usd
        notional = 0.0
        qty_sum = 0.0
        for px, qty in zip(prices, qtys):
            level_usd = float(px) * float(qty)
            take = min(remaining, level_usd)
            take_qty = take / float(px)
            notional += take
            qty_sum += take_qty
            remaining -= take
            if remaining <= 1e-9:
                break

        if qty_sum <= 0:
            return None
        vwap = notional / qty_sum
        if trade_side == "buy":
            slip_bps = (vwap - mid) / mid * 10_000
        else:
            slip_bps = (mid - vwap) / mid * 10_000
        return FillResult(
            side=trade_side,
            volume_usd=volume_usd,
            vwap=vwap,
            mid=mid,
            slippage_bps=float(slip_bps),
            filled_usd=notional,
            exhausted=remaining > 1.0,
        )

    def slippage_distribution(
        self,
        symbol: str,
        volume_usd: float,
        side: str = "buy",
        sample_every: int = 1,
    ) -> pd.Series:
        book = self._books.get(symbol)
        if not book:
            return pd.Series(dtype=float)
        ts_list = book["ts"][:: max(1, sample_every)]
        vals: List[float] = []
        for ts in ts_list[:: max(1, len(ts_list) // 200 or 1)]:
            fill = self.simulate_market_order(symbol, side, volume_usd, int(ts))
            if fill and not fill.exhausted:
                vals.append(fill.slippage_bps)
        return pd.Series(vals, name="slippage_bps")
