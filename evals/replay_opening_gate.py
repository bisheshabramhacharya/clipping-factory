#!/usr/bin/env python3
"""Replay the opening_strength >= 4 validator gate against a stored eval run.

Reads each sources/*/selection.json under a results directory and reports
which accepted candidates the current gate would keep or drop. Does not
re-transcribe or re-render. Exit 0 always; this is a report, not a CI gate.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def replay(run_dir: Path) -> dict:
    source_root = run_dir / "sources"
    warnings = []
    sources = sorted(path for path in source_root.iterdir() if path.is_dir()) if source_root.is_dir() else []
    if not source_root.is_dir():
        warnings.append(f"sources directory not found: {source_root}")
    rows = []
    kept = dropped = 0
    for src in sources:
        selection_path = src / "selection.json"
        if not selection_path.is_file():
            status = "unknown"
            result_path = src / "result.json"
            try:
                status = json.loads(result_path.read_text()).get("status", status)
            except (OSError, json.JSONDecodeError):
                pass
            warnings.append(f"{src.name}: selection.json unavailable (status: {status})")
            continue
        try:
            selection = json.loads(selection_path.read_text())
        except (OSError, json.JSONDecodeError) as exc:
            warnings.append(f"{src.name}: could not read selection.json: {exc}")
            continue
        for rank, item in enumerate(selection.get("accepted", []), start=1):
            cand = item.get("candidate") or item
            scores = cand.get("scores") or {}
            opening = scores.get("opening_strength")
            drop = opening is not None and opening < 4
            if drop:
                dropped += 1
            else:
                kept += 1
            rows.append(
                {
                    "source": src.name,
                    "rank": rank,
                    "opening_strength": opening,
                    "headline": cand.get("headline", ""),
                    "decision": "drop" if drop else "keep",
                }
            )
    return {
        "run_dir": str(run_dir),
        "accepted": kept + dropped,
        "kept": kept,
        "dropped": dropped,
        "clips": rows,
        "warnings": warnings,
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: replay_opening_gate.py <evals/results/<run-id>>", file=sys.stderr)
        return 2
    report = replay(Path(sys.argv[1]))
    json.dump(report, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
