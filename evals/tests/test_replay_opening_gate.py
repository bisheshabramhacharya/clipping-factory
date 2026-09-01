from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

PATH = Path(__file__).resolve().parents[1] / "replay_opening_gate.py"
SPEC = importlib.util.spec_from_file_location("replay_opening_gate", PATH)
assert SPEC and SPEC.loader
mod = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = mod
SPEC.loader.exec_module(mod)


class ReplayOpeningGateTests(unittest.TestCase):
    def test_reads_scores_nested_under_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "run"
            src = run / "sources" / "fast-accented"
            src.mkdir(parents=True)
            (src / "selection.json").write_text(
                json.dumps(
                    {
                        "selector": "local ranking",
                        "accepted": [
                            {
                                "candidate": {
                                    "headline": "good open",
                                    "scores": {"opening_strength": 5},
                                }
                            },
                            {
                                "candidate": {
                                    "headline": "It's the mothers",
                                    "scores": {"opening_strength": 3},
                                }
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )
            report = mod.replay(run)
        self.assertEqual(report["kept"], 1)
        self.assertEqual(report["dropped"], 1)
        self.assertEqual(report["clips"][1]["decision"], "drop")
        self.assertEqual(report["clips"][1]["headline"], "It's the mothers")
        self.assertEqual(report["warnings"], [])

    def test_failed_source_without_selection_is_a_warning(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "run"
            src = run / "sources" / "failed-source"
            src.mkdir(parents=True)
            (src / "result.json").write_text(
                json.dumps({"status": "failed", "error": "upload failed"}),
                encoding="utf-8",
            )
            report = mod.replay(run)
        self.assertEqual(report["accepted"], 0)
        self.assertEqual(len(report["warnings"]), 1)
        self.assertIn("status: failed", report["warnings"][0])


if __name__ == "__main__":
    unittest.main()
