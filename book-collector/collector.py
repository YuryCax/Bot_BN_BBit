#!/usr/bin/env python3
"""§8.7 book-collector — Binance depth + Bybit orderbook snapshots."""

from __future__ import annotations

import argparse
import asyncio
import json
from datetime import datetime, timezone
from pathlib import Path

import asyncpg
import websockets

SCHEMA = """
CREATE TABLE IF NOT EXISTS ob_snapshots (
    ts TIMESTAMPTZ NOT NULL,
    exchange TEXT NOT NULL,
    symbol TEXT NOT NULL,
    bid_depth_usd DOUBLE PRECISION,
    ask_depth_usd DOUBLE PRECISION,
    imbalance DOUBLE PRECISION,
    mid DOUBLE PRECISION
);
SELECT create_hypertable('ob_snapshots', 'ts', if_not_exists => TRUE);
"""


async def ensure_schema(dsn: str) -> asyncpg.Pool:
    pool = await asyncpg.create_pool(dsn)
    async with pool.acquire() as conn:
        try:
            await conn.execute(SCHEMA)
        except Exception:
            await conn.execute(
                """
                CREATE TABLE IF NOT EXISTS ob_snapshots (
                    ts TIMESTAMPTZ NOT NULL,
                    exchange TEXT NOT NULL,
                    symbol TEXT NOT NULL,
                    bid_depth_usd DOUBLE PRECISION,
                    ask_depth_usd DOUBLE PRECISION,
                    imbalance DOUBLE PRECISION,
                    mid DOUBLE PRECISION
                );
                """
            )
    return pool


async def collect_bybit(pool: asyncpg.Pool, symbol: str) -> None:
    url = "wss://stream.bybit.com/v5/public/linear"
    sub = {"op": "subscribe", "args": [f"orderbook.50.{symbol}"]}
    while True:
        try:
            async with websockets.connect(url) as ws:
                await ws.send(json.dumps(sub))
                async for raw in ws:
                    msg = json.loads(raw)
                    if not msg.get("topic", "").startswith("orderbook"):
                        continue
                    data = msg.get("data", {})
                    bids = data.get("b", [])
                    asks = data.get("a", [])
                    if not bids or not asks:
                        continue
                    bid_usd = sum(float(p) * float(q) for p, q in bids[:10])
                    ask_usd = sum(float(p) * float(q) for p, q in asks[:10])
                    mid = (float(bids[0][0]) + float(asks[0][0])) / 2
                    imb = (bid_usd - ask_usd) / max(bid_usd + ask_usd, 1.0)
                    ts = datetime.now(timezone.utc)
                    await pool.execute(
                        """
                        INSERT INTO ob_snapshots
                        (ts, exchange, symbol, bid_depth_usd, ask_depth_usd, imbalance, mid)
                        VALUES ($1, $2, $3, $4, $5, $6, $7)
                        """,
                        ts,
                        "bybit",
                        symbol,
                        bid_usd,
                        ask_usd,
                        imb,
                        mid,
                    )
        except Exception as exc:
            print(f"[bybit {symbol}] {exc}")
            await asyncio.sleep(2)


async def main_async(symbols: list[str], dsn: str) -> None:
    pool = await ensure_schema(dsn)
    await asyncio.gather(*[collect_bybit(pool, s) for s in symbols])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--symbols", nargs="+", default=["BTCUSDT", "ETHUSDT"])
    parser.add_argument(
        "--dsn",
        default="postgresql://bot:bot@localhost:5432/bot",
    )
    args = parser.parse_args()
    asyncio.run(main_async(args.symbols, args.dsn))


if __name__ == "__main__":
    main()
