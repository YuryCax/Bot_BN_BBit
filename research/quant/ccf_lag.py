#!/usr/bin/env python3
"""Cross-correlation lag analysis Binance → Bybit."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Tuple

import numpy as np
import pandas as pd


@dataclass
class CcfResult:
    symbol: str
    bar_ms: int
    best_lag_ms: int
    best_corr: float
    lags_ms: np.ndarray
    corrs: np.ndarray


def mid_from_trades(df: pd.DataFrame, exchange: str, bar_ms: int = 100) -> pd.Series:
    sub = df[df["exchange"] == exchange].copy()
    if sub.empty:
        return pd.Series(dtype=float)
    sub["bar"] = (sub["ts_ms"] // bar_ms) * bar_ms
    return sub.groupby("bar")["price"].last().astype(float)


def returns(series: pd.Series) -> pd.Series:
    return series.pct_change().dropna()


def cross_correlation(
    x: np.ndarray,
    y: np.ndarray,
    max_lag: int,
) -> Tuple[np.ndarray, np.ndarray]:
    """Pearson corr for lags where positive lag means y lags x (Bybit lags Binance)."""
    x = np.asarray(x, dtype=float)
    y = np.asarray(y, dtype=float)
    n = min(len(x), len(y))
    x = x[-n:]
    y = y[-n:]
    lags = np.arange(-max_lag, max_lag + 1)
    corrs = np.full(lags.shape, np.nan)
    for i, lag in enumerate(lags):
        if lag >= 0:
            a, b = x[: n - lag], y[lag:]
        else:
            a, b = x[-lag:], y[: n + lag]
        if len(a) < 10:
            continue
        if np.std(a) < 1e-12 or np.std(b) < 1e-12:
            continue
        corrs[i] = float(np.corrcoef(a, b)[0, 1])
    return lags, corrs


def calculate_optimal_lag(
    trades: pd.DataFrame,
    symbol: str,
    bar_ms: int = 100,
    max_lag_bars: int = 40,
) -> Optional[CcfResult]:
    sub = trades[trades["symbol"] == symbol]
    bn = mid_from_trades(sub, "binance", bar_ms)
    by = mid_from_trades(sub, "bybit", bar_ms)
    joined = pd.concat([bn.rename("bn"), by.rename("by")], axis=1).dropna()
    if len(joined) < 100:
        return None
    r_bn = returns(joined["bn"]).values
    r_by = returns(joined["by"]).values
    # Align after pct_change length mismatch
    m = min(len(r_bn), len(r_by))
    lags, corrs = cross_correlation(r_bn[-m:], r_by[-m:], max_lag_bars)
    if np.all(np.isnan(corrs)):
        return None
    # Prefer positive lag (Bybit behind Binance)
    pos_mask = lags >= 0
    if np.any(pos_mask & np.isfinite(corrs)):
        best_i = int(np.nanargmax(np.where(pos_mask, corrs, -np.inf)))
    else:
        best_i = int(np.nanargmax(corrs))
    best_lag_bars = int(lags[best_i])
    return CcfResult(
        symbol=symbol,
        bar_ms=bar_ms,
        best_lag_ms=best_lag_bars * bar_ms,
        best_corr=float(corrs[best_i]),
        lags_ms=lags * bar_ms,
        corrs=corrs,
    )


def maybe_plot(result: CcfResult, out_path: Path) -> None:
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        return
    out_path.parent.mkdir(parents=True, exist_ok=True)
    fig, ax = plt.subplots(figsize=(8, 4))
    ax.plot(result.lags_ms, result.corrs, lw=1.5)
    ax.axvline(result.best_lag_ms, color="red", ls="--", label=f"best={result.best_lag_ms}ms")
    ax.set_title(f"CCF Binance→Bybit {result.symbol}")
    ax.set_xlabel("lag ms (positive = Bybit lags)")
    ax.set_ylabel("correlation")
    ax.legend()
    ax.grid(True, alpha=0.3)
    fig.tight_layout()
    fig.savefig(out_path, dpi=120)
    plt.close(fig)
