from __future__ import annotations

import csv
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import score_moments  # noqa: E402


def moments(items):
    return {
        "schema_version": 1,
        "source_id": "ep1",
        "moments": [
            {
                "start_ms": s,
                "end_ms": e,
                "reason": r,
                "first_words": "one two three",
                "last_words": "four five six",
            }
            for s, e, r in items
        ],
    }


def selection(intervals):
    return {
        "selector": "local ranking",
        "accepted": [
            {
                "rank": i + 1,
                "composite": 10.0 - i,
                "candidate": {
                    "start_ms": s,
                    "end_ms": e,
                    "headline": f"clip {i}",
                    "opening_quote": "q",
                    "closing_quote": "q",
                    "selection_reason": "r",
                    "scores": {},
                },
            }
            for i, (s, e) in enumerate(intervals)
        ],
        "rejected": [],
    }


def write(path: Path, payload) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


def write_rubric(path: Path, rows) -> Path:
    fields = ["source", "clip_rank", "headline", *score_moments.RUBRIC_FIELDS,
              "decision_error", "reviewer", "notes"]
    with path.open("w", newline="", encoding="utf-8") as fh:
        writer = csv.writer(fh)
        writer.writerow(fields)
        writer.writerows(rows)
    return path


def rubric_row(source, rank, would_post=5):
    scores = {f: 4 for f in score_moments.RUBRIC_FIELDS}
    scores["would_post_1to5"] = would_post
    return [source, rank, "h", *[str(scores[f]) for f in score_moments.RUBRIC_FIELDS], "", "t", ""]


class OverlapTests(unittest.TestCase):
    def test_overlap_math(self):
        self.assertEqual(score_moments.overlap_ms((0, 10), (5, 15)), 5)
        self.assertEqual(score_moments.overlap_ms((0, 10), (10, 20)), 0)
        self.assertEqual(score_moments.overlap_ms((0, 10), (2, 4)), 2)

    def test_fifty_percent_boundary_counts(self):
        moment = [{"index": 0, "start_ms": 0, "end_ms": 20_000, "reason": "", "first_words": "a"}]
        accepted = [{"rank": 1, "start_ms": 10_000, "end_ms": 30_000,
                     "headline": "", "opening_quote": "", "composite": 1.0}]
        # exactly 50% of the moment covered
        self.assertEqual(score_moments.covered_moments(moment, accepted), [0])
        accepted[0]["start_ms"] = 10_001
        self.assertEqual(score_moments.covered_moments(moment, accepted), [])


class ScoreEpisodeTests(unittest.TestCase):
    def setUp(self):
        self.moments = [
            {"index": 0, "start_ms": 0, "end_ms": 20_000, "reason": "", "first_words": "a"},
            {"index": 1, "start_ms": 60_000, "end_ms": 90_000, "reason": "", "first_words": "b"},
            {"index": 2, "start_ms": 120_000, "end_ms": 150_000, "reason": "", "first_words": "c"},
        ]
        self.accepted = [
            {"rank": 1, "start_ms": 0, "end_ms": 25_000, "headline": "a", "opening_quote": "", "composite": 9.0},
            {"rank": 2, "start_ms": 62_000, "end_ms": 95_000, "headline": "b", "opening_quote": "", "composite": 8.0},
        ]

    def test_recall_and_misses(self):
        ep = score_moments.score_episode("ep1", self.moments, self.accepted, {})
        self.assertEqual(ep["recall"], round(2 / 3, 3))
        self.assertEqual(len(ep["missed"]), 1)
        self.assertEqual(ep["missed"][0]["start_ms"], 120_000)

    def test_precision_from_rubric(self):
        rubric = {("ep1", 1): {"would_post_1to5": 5}, ("ep1", 2): {"would_post_1to5": 2}}
        ep = score_moments.score_episode("ep1", self.moments, self.accepted, rubric)
        self.assertEqual(ep["judged_top_n"], 2)
        self.assertEqual(ep["precision"], 0.5)

    def test_precision_null_when_unjudged(self):
        ep = score_moments.score_episode("ep1", self.moments, self.accepted, {})
        self.assertIsNone(ep["precision"])
        self.assertEqual(ep["judged_top_n"], 0)

    def test_top_n_slicing_and_rank_order(self):
        many = [
            {"rank": i + 1, "start_ms": i * 10_000, "end_ms": i * 10_000 + 5000,
             "headline": "", "opening_quote": "", "composite": 1.0}
            for i in range(15)
        ]
        rubric = {("ep1", 15): {"would_post_1to5": 5}}
        ep = score_moments.score_episode("ep1", self.moments, many, rubric)
        # rank 15 is outside the top 10 and must not enter precision
        self.assertEqual(ep["judged_top_n"], 0)
        self.assertEqual(ep["top_n"], 10)


class LoadTests(unittest.TestCase):
    def test_moments_reject_overlong_quotes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp) / "m.json", moments([(0, 10_000, "r")]))
            doc = json.loads(path.read_text())
            doc["moments"][0]["first_words"] = "one two three four five six seven"
            path.write_text(json.dumps(doc))
            with self.assertRaises(RuntimeError):
                score_moments.load_moments(path)

    def test_moments_reject_bad_interval(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp) / "m.json", moments([(10_000, 0, "r")]))
            with self.assertRaises(RuntimeError):
                score_moments.load_moments(path)

    def test_selection_reads_accepted(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp) / "s.json", selection([(0, 30_000), (40_000, 70_000)]))
            rows = score_moments.load_selection(path)
            self.assertEqual([r["rank"] for r in rows], [1, 2])
            self.assertEqual(rows[0]["end_ms"], 30_000)


class RunDirTests(unittest.TestCase):
    def test_run_dir_aggregates(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "run"
            src = run / "sources" / "ep1"
            write(src / "selection.json", selection([(0, 30_000), (40_000, 70_000)]))
            (run / "sources" / "ep2").mkdir(parents=True)  # no selection -> skipped
            moments_dir = Path(tmp) / "moments"
            write(moments_dir / "ep1.moments.json", moments([(0, 20_000, "r"), (50_000, 80_000, "r")]))
            write_rubric(run / "rubric.csv", [rubric_row("ep1", 1, 5), rubric_row("ep1", 2, 4)])
            report = score_moments.score_run(run, moments_dir)
            self.assertEqual(report["totals"]["recall"], 1.0)
            self.assertEqual(report["totals"]["precision"], 1.0)
            self.assertEqual(report["skipped"], [{"source": "ep2", "reason": "no selection.json"}])

    def test_run_dir_requires_an_episode(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "run"
            (run / "sources" / "ep1").mkdir(parents=True)
            with self.assertRaises(RuntimeError):
                score_moments.score_run(run, Path(tmp))


if __name__ == "__main__":
    unittest.main()
