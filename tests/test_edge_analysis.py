import subprocess
import sys
import tomllib
from pathlib import Path

import pandas as pd


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "research" / "quant"))

from analyze_lead_lag import impulse_events_event_time


def test_synthetic_analysis_is_fail_closed(tmp_path: Path) -> None:
    profile = tmp_path / "edge_profile.toml"
    summary = tmp_path / "summary.md"
    data = tmp_path / "data"

    subprocess.run(
        [
            sys.executable,
            str(ROOT / "research" / "edge_report" / "analyze.py"),
            "--synthetic",
            "--data-dir",
            str(data),
            "--output-profile",
            str(profile),
            "--output-summary",
            str(summary),
        ],
        cwd=ROOT,
        check=True,
    )

    parsed = tomllib.loads(profile.read_text(encoding="utf-8"))
    meta = parsed["meta"]
    assert meta["status"] == "fail"
    assert meta["data_source"] == "synthetic"
    assert meta["research_method"] == "proxy_mid"
    assert meta["research_period_days"] < 14


def test_runtime_configs_remain_non_live() -> None:
    for name in ("config.toml", "config.testnet.toml", "config.production.toml"):
        parsed = tomllib.loads((ROOT / "config" / name).read_text(encoding="utf-8"))
        assert parsed["deployment"]["mode"] != "live"


def test_quant_events_apply_runtime_lag_residual_gate() -> None:
    trades = pd.DataFrame(
        [
            {"ts_ms": 0, "symbol": "BTCUSDT", "exchange": "binance", "price": 100.0},
            {"ts_ms": 100, "symbol": "BTCUSDT", "exchange": "binance", "price": 100.2},
            {"ts_ms": 200, "symbol": "BTCUSDT", "exchange": "binance", "price": 100.2},
            {"ts_ms": 0, "symbol": "BTCUSDT", "exchange": "bybit", "price": 100.0},
            {"ts_ms": 100, "symbol": "BTCUSDT", "exchange": "bybit", "price": 100.05},
            {"ts_ms": 250, "symbol": "BTCUSDT", "exchange": "bybit", "price": 100.15},
        ]
    )

    accepted = impulse_events_event_time(
        trades, "BTCUSDT", impulse_min_bps=12.0, lag_min_bps=10.0
    )
    rejected = impulse_events_event_time(
        trades, "BTCUSDT", impulse_min_bps=12.0, lag_min_bps=32.0
    )

    assert len(accepted) == 1
    assert rejected.empty
