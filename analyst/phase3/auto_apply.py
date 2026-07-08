#!/usr/bin/env python3
"""§10.7.4 Graduated auto-apply level manager."""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path

LEVEL_KINDS = {
    0: set(),
    1: {"trading_window", "halt_wallet", "pair_disable"},
    2: {"trading_window", "halt_wallet", "pair_disable", "config_patch", "pair_alloc"},
    3: {"trading_window", "halt_wallet", "pair_disable", "config_patch", "pair_alloc", "close_position"},
    4: {
        "trading_window",
        "halt_wallet",
        "pair_disable",
        "config_patch",
        "pair_alloc",
        "close_position",
        "manual_entry",
    },
}


def can_auto_apply(level: int, kind: str, kill_switch: bool) -> bool:
    if kill_switch:
        return False
    return kind in LEVEL_KINDS.get(level, set())


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, default=Path("config/analyst.toml"))
    parser.add_argument("--kind", required=True)
    args = parser.parse_args()
    data = tomllib.loads(args.config.read_text(encoding="utf-8"))
    p3 = data.get("phase3", {})
    level = int(p3.get("auto_apply_level", 0))
    kill = bool(p3.get("kill_switch", False))
    allowed = can_auto_apply(level, args.kind, kill)
    print(f"level={level} kind={args.kind} auto_apply={allowed}")


if __name__ == "__main__":
    main()
