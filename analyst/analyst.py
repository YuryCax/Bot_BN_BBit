#!/usr/bin/env python3
"""§8.6 Analyst Service — regime filter + Proposal builder (offline)."""

from __future__ import annotations

import argparse
import json
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

SUGGESTIONS_DIR = Path("analyst/data/suggestions")


def rule_based_regime(ema50: float, ema200: float, price: float) -> str:
    if ema50 > ema200 and price > ema50:
        return "uptrend"
    if ema50 < ema200 and price < ema50:
        return "downtrend"
    return "range"


def build_suggestion(kind: str, payload: dict, rationale: str) -> dict:
    now = datetime.now(timezone.utc)
    return {
        "id": str(uuid.uuid4()),
        "created_at": now.isoformat(),
        "expires_at": (now + timedelta(hours=24)).isoformat(),
        "status": "pending",
        "kind": kind,
        "payload": payload,
        "rationale": rationale,
        "confidence": 0.72,
        "source": "analyst",
    }


def run_analyst_scan() -> list[dict]:
    """Placeholder scan — production reads TimescaleDB + .bin logs."""
    suggestions = []
    regime = rule_based_regime(ema50=100.0, ema200=99.0, price=100.5)
    if regime == "range":
        suggestions.append(
            build_suggestion(
                "halt_wallet",
                {"wallet": "futures", "halt": True},
                "MA range/chop detected — reduce fee bleed",
            )
        )
    suggestions.append(
        build_suggestion(
            "trading_window",
            {"symbol": "BTCUSDT", "enabled_hours_utc": [13, 14, 15, 16, 17, 18]},
            "Align with edge_profile trade hours",
        )
    )
    return suggestions


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, default=SUGGESTIONS_DIR)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for sug in run_analyst_scan():
        path = args.output_dir / f"{sug['id']}.json"
        path.write_text(json.dumps(sug, indent=2), encoding="utf-8")
        print(f"Suggestion {sug['kind']} -> {path}")


if __name__ == "__main__":
    main()
