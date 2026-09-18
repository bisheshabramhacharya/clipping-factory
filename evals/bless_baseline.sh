#!/usr/bin/env bash
# Promote a finished eval run into a committed baseline (evals/baselines/<name>).
#
# Baselines are reports, not media: this copies report.json/report.csv/
# report.md and metadata.json — never clips, sources, or transcripts. Review
# the copied report before committing it; a baseline is the known-good a
# regression diff stands on.
#
# Usage: bash evals/bless_baseline.sh <run-dir> <baseline-name>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUN_DIR="${1:?usage: bless_baseline.sh <run-dir> <name>}"
NAME="${2:?usage: bless_baseline.sh <run-dir> <name>}"
[[ "$NAME" =~ ^[a-z0-9][a-z0-9._-]*$ ]] || { echo "baseline name must be lowercase slug" >&2; exit 1; }
[[ -f "$RUN_DIR/report.json" ]] || { echo "no report.json in $RUN_DIR — run report.py first" >&2; exit 1; }

DEST="$ROOT/evals/baselines/$NAME"
mkdir -p "$DEST"
for f in report.json report.csv report.md metadata.json; do
  [[ -f "$RUN_DIR/$f" ]] && cp "$RUN_DIR/$f" "$DEST/$f"
done
echo "baseline staged at $DEST — review report.md, then commit it"
