#!/usr/bin/env python3
"""Create deterministic JSON, CSV, and Markdown summaries for local eval runs."""
from __future__ import annotations

import argparse
import csv
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

SCORES = ("hook_1to5", "standalone_1to5", "payoff_1to5", "caption_accuracy_1to5", "framing_1to5", "would_post_1to5")
DEFAULTS = {"max_new_source_failures": 0, "max_would_post_rate_drop": 0.05, "max_average_score_drop": 0.25}


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"cannot read {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise RuntimeError(f"{path} must contain a JSON object")
    return value


def as_list(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []


def status(result: dict[str, Any], view: dict[str, Any]) -> str:
    project = view.get("project") if isinstance(view.get("project"), dict) else {}
    return str(result.get("status") or project.get("status") or "unknown")


def ready(view: dict[str, Any]) -> int:
    return sum(1 for clip in as_list(view.get("clips")) if isinstance(clip, dict) and clip.get("status") == "ready")


def accepted(view: dict[str, Any]) -> int:
    value = view.get("accepted")
    if isinstance(value, int):
        return max(value, 0)
    if isinstance(value, list):
        return len(value)
    return len(as_list(view.get("clips")))


def rejected_entries(view: dict[str, Any]) -> list[Any]:
    value = view.get("rejected")
    if isinstance(value, list):
        return value
    report = view.get("selection_report")
    if isinstance(report, dict):
        return as_list(report.get("rejected"))
    return []


def rejection_text(item: Any) -> str:
    if isinstance(item, str):
        return item
    if isinstance(item, dict):
        parts: list[str] = []
        for key in ("reason", "reasons", "rule", "message", "detail"):
            value = item.get(key)
            if isinstance(value, str):
                parts.append(value)
            elif isinstance(value, list):
                parts.extend(str(v) for v in value)
        return " ".join(parts)
    return str(item)


def load_sources(run_dir: Path) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    root = run_dir / "sources"
    if not root.is_dir():
        return output
    for folder in sorted(p for p in root.iterdir() if p.is_dir()):
        result_path = folder / "result.json"
        if not result_path.exists():
            continue
        result = read_json(result_path)
        view = read_json(folder / "view.json") if (folder / "view.json").exists() else {}
        rejects = rejected_entries(view)
        output.append({
            "source_id": str(result.get("source_id") or folder.name),
            "category": str(result.get("category") or "unspecified"),
            "status": status(result, view),
            "ready_clips": ready(view),
            "accepted_candidates": accepted(view),
            "rejected_candidates": len(rejects),
            "duplicate_rejections": sum(1 for x in rejects if any(t in rejection_text(x).lower() for t in ("overlap", "duplicate"))),
            "selector": str(view.get("selector") or "unknown"),
            "duration_seconds": result.get("duration_seconds"),
            "error": result.get("error"),
        })
    return output


def load_rubric(path: Path) -> dict[str, Any]:
    sums = Counter()
    counts = Counter()
    reviewed = would_post = false_accepts = false_rejects = 0
    invalid_rows: list[int] = []
    if not path.exists():
        return {"clips_reviewed": 0, "would_post_count": 0, "would_post_rate": None, "score_averages": {}, "false_accepts": 0, "false_rejects": 0, "invalid_rows": []}
    with path.open(newline="", encoding="utf-8") as handle:
        for row_number, row in enumerate(csv.DictReader(handle), start=2):
            if not any((row.get(field) or "").strip() for field in SCORES):
                continue
            parsed: dict[str, float] = {}
            valid = True
            for field in SCORES:
                text = (row.get(field) or "").strip()
                if not text:
                    continue
                try:
                    value = float(text)
                except ValueError:
                    valid = False
                    break
                if not 1 <= value <= 5:
                    valid = False
                    break
                parsed[field] = value
            if not valid or not parsed:
                invalid_rows.append(row_number)
                continue
            reviewed += 1
            for field, value in parsed.items():
                sums[field] += value
                counts[field] += 1
            if parsed.get("would_post_1to5", 0) >= 4:
                would_post += 1
            error = (row.get("decision_error") or "").strip().lower()
            false_accepts += error == "false_accept"
            false_rejects += error == "false_reject"
    averages = {field: round(sums[field] / counts[field], 3) for field in SCORES if counts[field]}
    return {
        "clips_reviewed": reviewed,
        "would_post_count": would_post,
        "would_post_rate": round(would_post / reviewed, 3) if reviewed else None,
        "score_averages": averages,
        "false_accepts": false_accepts,
        "false_rejects": false_rejects,
        "invalid_rows": invalid_rows,
    }


def aggregate(run_dir: Path) -> dict[str, Any]:
    metadata = read_json(run_dir / "metadata.json")
    sources = load_sources(run_dir)
    total_candidates = sum(s["accepted_candidates"] + s["rejected_candidates"] for s in sources)
    duplicates = sum(s["duplicate_rejections"] for s in sources)
    states = Counter(s["status"] for s in sources)
    operational = {
        "source_count": len(sources),
        "completed_sources": states["complete"],
        "failed_sources": states["failed"],
        "cancelled_sources": states["cancelled"],
        "ready_clips": sum(s["ready_clips"] for s in sources),
        "accepted_candidates": sum(s["accepted_candidates"] for s in sources),
        "rejected_candidates": sum(s["rejected_candidates"] for s in sources),
        "duplicate_rejections": duplicates,
        "duplicate_rate": round(duplicates / total_candidates, 3) if total_candidates else None,
        "total_duration_seconds": round(sum(float(s["duration_seconds"]) for s in sources if isinstance(s["duration_seconds"], (int, float))), 3),
        "selector_counts": dict(sorted(Counter(s["selector"] for s in sources).items())),
        "category_counts": dict(sorted(Counter(s["category"] for s in sources).items())),
    }
    return {"schema_version": 1, "run": metadata, "operational": operational, "human_review": load_rubric(run_dir / "rubric.csv"), "sources": sources}


def load_report(path: Path) -> dict[str, Any]:
    return read_json(path / "report.json") if path.is_dir() and (path / "report.json").exists() else aggregate(path) if path.is_dir() else read_json(path)


def compare(current: dict[str, Any], baseline: dict[str, Any], thresholds: dict[str, Any] | None = None) -> dict[str, Any]:
    policy = DEFAULTS | (thresholds or {})
    reasons: list[str] = []
    warnings: list[str] = []
    co, bo = current["operational"], baseline["operational"]
    new_failures = max(int(co["failed_sources"]) - int(bo["failed_sources"]), 0)
    if new_failures > int(policy["max_new_source_failures"]):
        reasons.append(f"new source failures: {new_failures}")
    cr, br = current["human_review"], baseline["human_review"]
    if cr["would_post_rate"] is not None and br["would_post_rate"] is not None:
        drop = round(float(br["would_post_rate"]) - float(cr["would_post_rate"]), 3)
        if drop > float(policy["max_would_post_rate_drop"]):
            reasons.append(f"would-post rate dropped by {drop:.3f}")
    else:
        warnings.append("would-post comparison unavailable until both runs are reviewed")
    for field, baseline_value in br["score_averages"].items():
        current_value = cr["score_averages"].get(field)
        if current_value is None:
            warnings.append(f"{field} comparison unavailable")
            continue
        drop = round(float(baseline_value) - float(current_value), 3)
        if drop > float(policy["max_average_score_drop"]):
            reasons.append(f"{field} average dropped by {drop:.3f}")
    return {"status": "fail" if reasons else "pass", "baseline_run_id": baseline["run"].get("run_id"), "thresholds": policy, "failure_reasons": reasons, "warnings": warnings}


def md_cell(value: Any) -> str:
    return str(value if value not in (None, "") else "—").replace("\\", "\\\\").replace("|", "\\|").replace("\n", " ")


def render_md(report: dict[str, Any]) -> str:
    run, op, review = report["run"], report["operational"], report["human_review"]
    lines = [f"# Clipping Factory eval — {md_cell(run.get('run_id'))}", "", f"- Commit: `{md_cell(run.get('git_commit'))}`", f"- Branch: `{md_cell(run.get('git_branch'))}`", "", "## Operational", "", "| Metric | Value |", "|---|---:|"]
    for key in ("source_count", "completed_sources", "failed_sources", "ready_clips", "accepted_candidates", "rejected_candidates", "duplicate_rate", "total_duration_seconds"):
        lines.append(f"| {key} | {md_cell(op.get(key))} |")
    lines += ["", "## Human review", "", f"- Clips reviewed: {review['clips_reviewed']}", f"- Would-post rate: {md_cell(review['would_post_rate'])}", f"- False accepts / rejects: {review['false_accepts']} / {review['false_rejects']}"]
    for field, value in review["score_averages"].items():
        lines.append(f"- {field}: {value}")
    comparison = report.get("comparison")
    if isinstance(comparison, dict):
        lines += ["", "## Baseline gate", "", f"**{str(comparison['status']).upper()}**"]
        lines += [f"- {reason}" for reason in comparison["failure_reasons"]]
        lines += [f"- Warning: {warning}" for warning in comparison["warnings"]]
    lines += ["", "## Source results", "", "| Source | Category | Status | Ready | Rejected | Selector | Error |", "|---|---|---|---:|---:|---|---|"]
    for source in report["sources"]:
        lines.append("| {source_id} | {category} | {status} | {ready_clips} | {rejected_candidates} | {selector} | {error} |".format(**{k: md_cell(v) for k, v in source.items()}))
    if review["invalid_rows"]:
        lines += ["", f"> Invalid rubric rows ignored: {', '.join(map(str, review['invalid_rows']))}"]
    return "\n".join(lines) + "\n"


def render_csv(report: dict[str, Any]) -> str:
    op, review = report["operational"], report["human_review"]
    fields = ["run_id", "git_commit", "source_count", "completed_sources", "failed_sources", "ready_clips", "accepted_candidates", "rejected_candidates", "duplicate_rate", "clips_reviewed", "would_post_rate", "gate_status"]
    row = {"run_id": report["run"].get("run_id"), "git_commit": report["run"].get("git_commit"), **{k: op.get(k) for k in fields}, "clips_reviewed": review["clips_reviewed"], "would_post_rate": review["would_post_rate"], "gate_status": (report.get("comparison") or {}).get("status")}
    from io import StringIO
    out = StringIO(newline="")
    writer = csv.DictWriter(out, fieldnames=fields, lineterminator="\n")
    writer.writeheader(); writer.writerow({k: "" if row.get(k) is None else row.get(k) for k in fields})
    return out.getvalue()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("run_dir", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--thresholds", type=Path)
    parser.add_argument("--output-json", type=Path)
    parser.add_argument("--output-md", type=Path)
    parser.add_argument("--output-csv", type=Path)
    parser.add_argument("--enforce", action="store_true")
    args = parser.parse_args(argv)
    try:
        report = aggregate(args.run_dir.resolve())
        if args.baseline:
            policy = read_json(args.thresholds) if args.thresholds else None
            report["comparison"] = compare(report, load_report(args.baseline.resolve()), policy)
        json_path = args.output_json or args.run_dir / "report.json"
        md_path = args.output_md or args.run_dir / "report.md"
        csv_path = args.output_csv or args.run_dir / "report.csv"
        for path in (json_path, md_path, csv_path):
            path.parent.mkdir(parents=True, exist_ok=True)
        json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        md_path.write_text(render_md(report), encoding="utf-8")
        csv_path.write_text(render_csv(report), encoding="utf-8")
    except RuntimeError as exc:
        print(f"eval report error: {exc}", file=sys.stderr)
        return 1
    print(f"Wrote {json_path}, {csv_path}, and {md_path}")
    return 2 if args.enforce and (report.get("comparison") or {}).get("status") == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
