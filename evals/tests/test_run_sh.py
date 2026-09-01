from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

RUNNER = Path(__file__).resolve().parents[1] / "run.sh"


class EvalRunnerTests(unittest.TestCase):
    def fake_path(self, root: Path, marker: Path) -> str:
        bin_dir = root / "bin"
        bin_dir.mkdir()
        (bin_dir / "curl").write_text(
            f"#!/bin/sh\ntouch {marker!s}\nexit 99\n", encoding="utf-8"
        )
        (bin_dir / "jq").write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
        (bin_dir / "curl").chmod(0o755)
        (bin_dir / "jq").chmod(0o755)
        return f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}"

    def test_malicious_numeric_input_fails_before_network(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "curl-called"
            result = subprocess.run(
                ["/bin/bash", str(RUNNER), "--poll-seconds", "1; touch /tmp/nope"],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env={**os.environ, "PATH": self.fake_path(root, marker)},
                check=False,
            )
            curl_called = marker.exists()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("positive integer", result.stderr)
        self.assertFalse(curl_called)

    def test_resume_manifest_mismatch_fails_before_network(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            marker = root / "curl-called"
            supplied = root / "manifest.json"
            supplied.write_text(
                json.dumps({"schema_version": 1, "sources": [{"id": "a", "path": "a.mp4"}]}),
                encoding="utf-8",
            )
            run = root / "run"
            run.mkdir()
            snapshot = run / "manifest.json"
            snapshot.write_text(
                json.dumps({"schema_version": 1, "sources": [{"id": "b", "path": "b.mp4"}]}),
                encoding="utf-8",
            )
            digest = hashlib.sha256(supplied.read_bytes()).hexdigest()
            (run / "metadata.json").write_text(
                json.dumps({"schema_version": 1, "run_id": "run", "manifest_sha256": digest}),
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    "/bin/bash",
                    str(RUNNER),
                    "--manifest",
                    str(supplied),
                    "--run-dir",
                    str(run),
                    "--resume",
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env={**os.environ, "PATH": self.fake_path(root, marker)},
                check=False,
            )
            curl_called = marker.exists()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("manifest hash mismatch", result.stderr)
        self.assertFalse(curl_called)


if __name__ == "__main__":
    unittest.main()
