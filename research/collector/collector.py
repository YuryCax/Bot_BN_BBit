#!/usr/bin/env python3
"""§9.0 Edge Research collector — Binance Futures + Bybit Linear mids @ 100ms."""

from __future__ import annotations

import argparse
import asyncio
import json
import signal
import time
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Deque, Dict, List, Optional, Tuple

import pandas as pd
import websockets

BINANCE_WS = "wss://fstream.binance.com/stream"
BYBIT_WS = "wss://stream.bybit.com/v5/public/linear"

SAMPLE_MS = 100
FORWARD_MS = (200, 500, 1000)


@dataclass
class SymbolState:
    symbol: str
    binance_mid: Optional[float] = None
    bybit_mid: Optional[float] = None
    mid_history: Deque[Tuple[int, float]] = field(default_factory=lambda: deque(maxlen=2000))
    rows: List[dict] = field(default_factory=list)
    pending_impulses: Deque[dict] = field(default_factory=deque)


def utc_ms() -> int:
    return int(time.time() * 1000)


def impulse_bps_100ms(history: Deque[Tuple[int, float]], now_ms: int, mid: float) -> float:
    target = now_ms - 100
    ref = None
    for ts, p in reversed(history):
        if ts <= target:
            ref = p
            break
    if ref is None or ref <= 0:
        return 0.0
    return (mid - ref) / ref * 10_000


def lag_bps(binance_mid: float, bybit_mid: float) -> float:
    if bybit_mid <= 0:
        return 0.0
    return (binance_mid - bybit_mid) / bybit_mid * 10_000


def resolve_forwards(state: SymbolState, now_ms: int) -> None:
    while state.pending_impulses and now_ms - state.pending_impulses[0]["ts_ms"] > max(FORWARD_MS):
        imp = state.pending_impulses.popleft()
        b_mid = state.bybit_mid
        if b_mid is None:
            continue
        base = imp["bybit_mid_at_impulse"]
        if base <= 0:
            continue
        ret_bps = (b_mid - base) / base * 10_000
        direction = 1 if imp["impulse_bps_100ms"] > 0 else -1
        aligned = ret_bps * direction > 0
        imp.update(
            {
                "fwd_ret_bybit_200ms": ret_bps if now_ms - imp["ts_ms"] >= 200 else None,
                "fwd_ret_bybit_500ms": ret_bps if now_ms - imp["ts_ms"] >= 500 else None,
                "fwd_ret_bybit_1000ms": ret_bps if now_ms - imp["ts_ms"] >= 1000 else None,
                "follow_through": aligned,
            }
        )
        if now_ms - imp["ts_ms"] >= 1000:
            state.rows.append(imp)


async def binance_loop(symbols: List[str], states: Dict[str, SymbolState]) -> None:
    streams = "/".join(f"{s.lower()}@bookTicker" for s in symbols)
    url = f"{BINANCE_WS}?streams={streams}"
    while True:
        try:
            async with websockets.connect(url, ping_interval=20) as ws:
                async for raw in ws:
                    msg = json.loads(raw)
                    data = msg.get("data", msg)
                    sym = data.get("s")
                    if sym not in states:
                        continue
                    bid = float(data["b"])
                    ask = float(data["a"])
                    states[sym].binance_mid = (bid + ask) / 2
        except Exception as exc:
            print(f"[binance] reconnect after error: {exc}")
            await asyncio.sleep(2)


async def bybit_loop(symbols: List[str], states: Dict[str, SymbolState]) -> None:
    sub = {"op": "subscribe", "args": [f"orderbook.1.{s}" for s in symbols]}
    while True:
        try:
            async with websockets.connect(BYBIT_WS, ping_interval=20) as ws:
                await ws.send(json.dumps(sub))
                async for raw in ws:
                    msg = json.loads(raw)
                    if msg.get("topic", "").startswith("orderbook.1."):
                        sym = msg["topic"].split(".")[-1]
                        if sym not in states:
                            continue
                        data = msg.get("data", {})
                        bids = data.get("b", [])
                        asks = data.get("a", [])
                        if not bids or not asks:
                            continue
                        bid = float(bids[0][0])
                        ask = float(asks[0][0])
                        states[sym].bybit_mid = (bid + ask) / 2
        except Exception as exc:
            print(f"[bybit] reconnect after error: {exc}")
            await asyncio.sleep(2)


async def sampler_loop(
    states: Dict[str, SymbolState],
    impulse_min_bps: float,
    stop_event: asyncio.Event,
) -> None:
    while not stop_event.is_set():
        now = utc_ms()
        dt = datetime.fromtimestamp(now / 1000, tz=timezone.utc)
        hour_utc = dt.hour
        for sym, st in states.items():
            if st.binance_mid is None or st.bybit_mid is None:
                continue
            b_mid = st.binance_mid
            y_mid = st.bybit_mid
            st.mid_history.append((now, b_mid))
            imp = impulse_bps_100ms(st.mid_history, now, b_mid)
            row = {
                "ts_ms": now,
                "symbol": sym,
                "hour_utc": hour_utc,
                "binance_mid": b_mid,
                "bybit_mid": y_mid,
                "lag_bps": lag_bps(b_mid, y_mid),
                "impulse_bps_100ms": imp,
                "atr_pct": abs(imp) / 100,  # proxy for vol bucket
            }
            st.rows.append(row)
            if abs(imp) >= impulse_min_bps:
                st.pending_impulses.append(
                    {
                        **row,
                        "bybit_mid_at_impulse": y_mid,
                        "follow_through": None,
                        "fwd_ret_bybit_200ms": None,
                        "fwd_ret_bybit_500ms": None,
                        "fwd_ret_bybit_1000ms": None,
                    }
                )
            resolve_forwards(st, now)
        await asyncio.sleep(SAMPLE_MS / 1000)


async def run_collector(
    symbols: List[str],
    output_dir: Path,
    duration_sec: Optional[int],
    impulse_min_bps: float,
) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    states = {s: SymbolState(symbol=s) for s in symbols}
    stop = asyncio.Event()

    def _stop(*_: object) -> None:
        stop.set()

    try:
        signal.signal(signal.SIGINT, _stop)
        signal.signal(signal.SIGTERM, _stop)
    except ValueError:
        pass

    tasks = [
        asyncio.create_task(binance_loop(symbols, states)),
        asyncio.create_task(bybit_loop(symbols, states)),
        asyncio.create_task(sampler_loop(states, impulse_min_bps, stop)),
    ]

    start = time.time()
    while not stop.is_set():
        if duration_sec and time.time() - start >= duration_sec:
            stop.set()
            break
        await asyncio.sleep(0.5)

    for t in tasks:
        t.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)

    for sym, st in states.items():
        if not st.rows:
            continue
        df = pd.DataFrame(st.rows)
        out = output_dir / f"{sym}_{datetime.now(timezone.utc).strftime('%Y%m%d_%H%M%S')}.parquet"
        df.to_parquet(out, index=False)
        print(f"Wrote {len(df)} rows -> {out}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Edge Research collector §9.0")
    parser.add_argument("--symbols", nargs="+", default=["BTCUSDT", "ETHUSDT"])
    parser.add_argument("--output", type=Path, default=Path("research/data"))
    parser.add_argument("--duration-sec", type=int, default=None, help="Stop after N seconds")
    parser.add_argument("--impulse-min-bps", type=float, default=5.0)
    args = parser.parse_args()
    asyncio.run(
        run_collector(args.symbols, args.output, args.duration_sec, args.impulse_min_bps)
    )


if __name__ == "__main__":
    main()
