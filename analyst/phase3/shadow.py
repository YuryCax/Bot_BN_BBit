#!/usr/bin/env python3
"""§10.7.3 Shadow mode — counterfactual PF tracker."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def compute_shadow_metrics(outcomes: list[dict]) -> dict:
    actual = [o["actual_pnl_pct"] for o in outcomes]
    shadow = [o["would_have_pnl_pct"] for o in outcomes]
    pf_actual = _profit_factor(actual)
    pf_shadow = _profit_factor(shadow)
    exp_shadow = sum(shadow) / len(shadow) if shadow else 0.0
    return {
        "pf_actual": pf_actual,
        "pf_shadow": pf_shadow,
        "expectancy_shadow": exp_shadow,
        "pass": pf_shadow > pf_actual and exp_shadow > 0,
    }


def _profit_factor(pnls: list[float]) -> float:
    wins = sum(p for p in pnls if p > 0)
    losses = abs(sum(p for p in pnls if p < 0))
    if losses == 0:
        return wins
    return wins / losses


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=Path("analyst/data/shadow_outcomes.json"))
    args = parser.parse_args()
    if not args.input.exists():
        sample = [
            {"actual_pnl_pct": 0.1, "would_have_pnl_pct": 0.15},
            {"actual_pnl_pct": -0.05, "would_have_pnl_pct": 0.02},
            {"actual_pnl_pct": 0.08, "would_have_pnl_pct": 0.12},
        ]
        args.input.parent.mkdir(parents=True, exist_ok=True)
        args.input.write_text(json.dumps(sample, indent=2), encoding="utf-8")
    outcomes = json.loads(args.input.read_text(encoding="utf-8"))
    metrics = compute_shadow_metrics(outcomes)
    print(json.dumps(metrics, indent=2))


if __name__ == "__main__":
    main()
