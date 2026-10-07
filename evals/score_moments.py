#!/usr/bin/env python3
"""Score a selection against an episode's moments answer key.

Two numbers decide every ranking change:

- recall    — the share of answer-key moments that some accepted candidate
              covers (>= 50% interval overlap)
- precision — the share of the top-10 accepted candidates a reviewer would
              post, judged with the run's rubric.csv (would_post_1to5 >= 4)

Usage:

  # one episode
  python3 evals/score_moments.py --selection selection.json \
      --moments evals/public/<source-id>.moments.json [--rubric rubric.csv]

  # every episode in a run directory
  python3 evals/score_moments.py --run-dir evals/results/<run-id>

Moments files live in git at evals/public/<source-id>.moments.json and hold
times, a reason, and at most the first and last six words of each moment —
never transcript text.
"""
from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path
from typing import Any

OVERLAP_MIN = 0.5
TOP_N = 10
WOULD_POST_MIN = 4
RUBRIC_FIELDS = (
    "hook_1to5",
    "standalone_1to5",
    "payoff_1to5",
    "caption_accuracy_1to5",
    "framing_1to5",
    "would_post_1to5",
)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"cannot read {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise RuntimeError(f"{path} must contain a JSON object")
    return value


def interval_ms(item: dict[str, Any]) -> tuple[int, int]:
    start, end = item.get("start_ms"), item.get("end_ms")
    if not isinstance(start, int) or not isinstance(end, int) or end <= start:
        raise RuntimeError(f"bad interval: {item!r}")
    return start, end


def overlap_ms(a: tuple[int, int], b: tuple[int, int]) -> int:
    return max(0, min(a[1], b[1]) - max(a[0], b[0]))


def load_moments(path: Path) -> list[dict[str, Any]]:
    data = read_json(path)
    if data.get("schema_version") != 1:
        raise RuntimeError(f"{path}: schema_version must be 1")
    moments = data.get("moments")
    if not isinstance(moments, list) or not moments:
        raise RuntimeError(f"{path}: moments must be a non-empty list")
    out: list[dict[str, Any]] = []
    for i, m in enumerate(moments):
        if not isinstance(m, dict):
            raise RuntimeError(f"{path}: moment {i} must be an object")
        start, end = interval_ms(m)
        for field in ("first_words", "last_words"):
            words = str(m.get(field) or "").split()
            if not 1 <= len(words) <= 6:
                raise RuntimeError(f"{path}: moment {i} {field} must be 1-6 words")
        out.append({"index": i, "start_ms": start, "end_ms": end,
                    "reason": str(m.get("reason") or ""),
                    "first_words": str(m.get("first_words") or "")})
    return out


def load_selection(path: Path) -> list[dict[str, Any]]:
    data = read_json(path)
    accepted = data.get("accepted")
    if not isinstance(accepted, list):
        raise RuntimeError(f"{path}: accepted must be a list")
    rows = []
    for vc in accepted:
        if not isinstance(vc, dict) or not isinstance(vc.get("candidate"), dict):
            raise RuntimeError(f"{path}: accepted entries need a candidate object")
        cand = vc["candidate"]
        start, end = interval_ms(cand)
        rows.append({
            "rank": vc.get("rank"),
            "start_ms": start,
            "end_ms": end,
            "headline": str(cand.get("headline") or ""),
            "opening_quote": str(cand.get("opening_quote") or ""),
            "composite": vc.get("composite"),
        })
    return rows


def load_rubric(path: Path) -> dict[tuple[str, int], dict[str, Any]]:
    """(source, clip_rank) -> parsed row; only rows with every score field."""
    out: dict[tuple[str, int], dict[str, Any]] = {}
    if not path.exists():
        return out
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            source = (row.get("source") or "").strip()
            rank_text = (row.get("clip_rank") or "").strip()
            if not source or not rank_text:
                continue
            try:
                rank = int(rank_text)
            except ValueError:
                continue
            scores: dict[str, float] = {}
            valid = True
            for field in RUBRIC_FIELDS:
                text = (row.get(field) or "").strip()
                if not text:
                    valid = False
                    break
                try:
                    value = float(text)
                except ValueError:
                    valid = False
                    break
                if not 1 <= value <= 5:
                    valid = False
                    break
                scores[field] = value
            if valid:
                out[(source, rank)] = scores
    return out


def covered_moments(moments: list[dict[str, Any]], accepted: list[dict[str, Any]]) -> list[int]:
    covered = []
    for m in moments:
        span = (m["start_ms"], m["end_ms"])
        length = span[1] - span[0]
        if any(overlap_ms(span, (c["start_ms"], c["end_ms"])) >= length * OVERLAP_MIN for c in accepted):
            covered.append(m["index"])
    return covered


def score_episode(
    source_id: str,
    moments: list[dict[str, Any]],
    accepted: list[dict[str, Any]],
    rubric: dict[tuple[str, int], dict[str, Any]],
) -> dict[str, Any]:
    covered = covered_moments(moments, accepted)
    missed = [m for m in moments if m["index"] not in covered]
    ordered = sorted(accepted, key=lambda r: r["rank"] if isinstance(r["rank"], int) else 10_000)
    top = ordered[:TOP_N]
    judged = [r for r in top if (source_id, r["rank"]) in rubric]
    posts = [r for r in judged if rubric[(source_id, r["rank"])]["would_post_1to5"] >= WOULD_POST_MIN]
    return {
        "source_id": source_id,
        "moments": len(moments),
        "covered": len(covered),
        "recall": round(len(covered) / len(moments), 3),
        "missed": [{"start_ms": m["start_ms"], "end_ms": m["end_ms"],
                    "first_words": m["first_words"], "reason": m["reason"]} for m in missed],
        "accepted": len(accepted),
        "top_n": len(top),
        "judged_top_n": len(judged),
        "would_post_top_n": len(posts),
        "precision": round(len(posts) / len(judged), 3) if judged else None,
    }


def score_run(run_dir: Path, moments_dir: Path) -> dict[str, Any]:
    sources_dir = run_dir / "sources"
    if not sources_dir.is_dir():
        raise RuntimeError(f"{run_dir} has no sources/ directory")
    rubric = load_rubric(run_dir / "rubric.csv")
    episodes = []
    skipped = []
    for folder in sorted(p for p in sources_dir.iterdir() if p.is_dir()):
        selection_path = folder / "selection.json"
        moments_path = moments_dir / f"{folder.name}.moments.json"
        if not selection_path.exists():
            skipped.append((folder.name, "no selection.json"))
            continue
        if not moments_path.exists():
            skipped.append((folder.name, "no answer key"))
            continue
        episodes.append(score_episode(
            folder.name,
            load_moments(moments_path),
            load_selection(selection_path),
            rubric,
        ))
    if not episodes:
        raise RuntimeError(f"no episodes scored under {sources_dir}")
    total_moments = sum(e["moments"] for e in episodes)
    total_covered = sum(e["covered"] for e in episodes)
    judged = sum(e["judged_top_n"] for e in episodes)
    posts = sum(e["would_post_top_n"] for e in episodes)
    return {
        "schema_version": 1,
        "run_id": run_dir.name,
        "episodes": episodes,
        "skipped": [{"source": s, "reason": r} for s, r in skipped],
        "totals": {
            "episodes": len(episodes),
            "moments": total_moments,
            "covered": total_covered,
            "recall": round(total_covered / total_moments, 3),
            "judged_top_n": judged,
            "would_post_top_n": posts,
            "precision": round(posts / judged, 3) if judged else None,
        },
    }


def fmt_ms(ms: int) -> str:
    total, millis = divmod(ms, 1000)
    minutes, seconds = divmod(total, 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours:d}:{minutes:02d}:{seconds:02d}.{millis:03d}"


def render_table(report: dict[str, Any]) -> str:
    lines = [
        "| source | moments | covered | recall | top10 | judged | would-post | precision |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for e in report["episodes"]:
        precision = "—" if e["precision"] is None else f"{e['precision']:.3f}"
        lines.append(
            f"| {e['source_id']} | {e['moments']} | {e['covered']} | {e['recall']:.3f} "
            f"| {e['top_n']} | {e['judged_top_n']} | {e['would_post_top_n']} "
            f"| {precision} |"
        )
    t = report["totals"]
    total_precision = "—" if t["precision"] is None else f"{t['precision']:.3f}"
    lines.append(
        f"| **total** | {t['moments']} | {t['covered']} | {t['recall']:.3f} "
        f"| — | {t['judged_top_n']} | {t['would_post_top_n']} | {total_precision} |"
    )
    for e in report["episodes"]:
        if e["missed"]:
            lines += ["", f"Missed moments in {e['source_id']}:"]
            for m in e["missed"]:
                lines.append(
                    f"- {fmt_ms(m['start_ms'])}–{fmt_ms(m['end_ms'])} "
                    f"“{m['first_words']}…” — {m['reason']}"
                )
    if report["skipped"]:
        lines += ["", "Skipped (missing selection or answer key):"]
        lines += [f"- {s['source']}: {s['reason']}" for s in report["skipped"]]
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-dir", type=Path, help="evals/results/<run-id> directory")
    parser.add_argument("--selection", type=Path, help="one episode's selection.json")
    parser.add_argument("--moments", type=Path, help="one episode's moments.json")
    parser.add_argument("--rubric", type=Path, help="rubric.csv (default: alongside --selection)")
    parser.add_argument("--moments-dir", type=Path, default=Path(__file__).parent / "public",
                        help="answer-key directory for --run-dir (default: evals/public)")
    parser.add_argument("--source-id", help="rubric source id for --selection (default: moments file's source_id)")
    parser.add_argument("--json", type=Path, help="write the full report JSON here")
    args = parser.parse_args(argv)

    try:
        if args.run_dir:
            report = score_run(args.run_dir.resolve(), args.moments_dir.resolve())
        elif args.selection and args.moments:
            moments_path = args.moments.resolve()
            moments_doc = read_json(moments_path)
            source_id = args.source_id or str(moments_doc.get("source_id") or args.selection.stem)
            rubric_path = args.rubric or args.selection.parent / "rubric.csv"
            episode = score_episode(
                source_id,
                load_moments(moments_path),
                load_selection(args.selection.resolve()),
                load_rubric(rubric_path),
            )
            report = {
                "schema_version": 1,
                "run_id": source_id,
                "episodes": [episode],
                "skipped": [],
                "totals": {
                    "episodes": 1,
                    "moments": episode["moments"],
                    "covered": episode["covered"],
                    "recall": episode["recall"],
                    "judged_top_n": episode["judged_top_n"],
                    "would_post_top_n": episode["would_post_top_n"],
                    "precision": episode["precision"],
                },
            }
        else:
            parser.error("pass either --run-dir or --selection + --moments")
            return 2
        sys.stdout.write(render_table(report))
        if args.json:
            args.json.parent.mkdir(parents=True, exist_ok=True)
            args.json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except RuntimeError as exc:
        print(f"score_moments error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
