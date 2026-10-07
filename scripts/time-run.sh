#!/usr/bin/env bash
# Upload a source to a running studio, wait for the pipeline, and print
# wall-clock timings: per-stage seconds (project.json via the view API)
# and per-clip render times (observed status transitions).
#
# Every invocation creates a fresh project, so repeated runs re-time the
# whole pipeline cold. Nothing is deleted; old projects stay in the studio
# library until removed by hand.
#
# Usage: scripts/time-run.sh <video> [--port N] [--runs N] [--poll S]
#   <video>      source MP4 to upload
#   --port N     studio port (default ${CF_PORT:-4571})
#   --runs N     repeat the run N times (default 1)
#   --poll S     seconds between status polls (default 2)
#
# Requires: curl, jq, and a studio already running (cargo run --release).

set -euo pipefail

VIDEO=""
PORT="${CF_PORT:-4571}"
RUNS=1
POLL=2

while (($#)); do
  case "$1" in
    --port|--runs|--poll)
      (($# >= 2)) || { echo "$1 requires a value" >&2; exit 1; }
      option="$1"; value="$2"; shift 2
      case "$option" in
        --port) PORT="$value" ;;
        --runs) RUNS="$value" ;;
        --poll) POLL="$value" ;;
      esac
      ;;
    -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
    -*) echo "Unknown option: $1" >&2; exit 1 ;;
    *) [[ -z "$VIDEO" ]] && VIDEO="$1" || { echo "One video argument only" >&2; exit 1; }; shift ;;
  esac
done

[[ -n "$VIDEO" && -f "$VIDEO" ]] || { echo "Usage: $0 <video> [--port N] [--runs N] [--poll S]" >&2; exit 1; }
for cmd in curl jq python3; do
  command -v "$cmd" >/dev/null || { echo "$cmd is required" >&2; exit 1; }
done
[[ "$RUNS" =~ ^[1-9][0-9]*$ ]] || { echo "--runs must be a positive integer" >&2; exit 1; }

HOST="http://127.0.0.1:$PORT"
curl -sf --max-time 5 "$HOST/api/setup" >/dev/null \
  || { echo "No studio on $HOST — start one with: cargo run --release" >&2; exit 1; }

# One full upload-to-done run; prints a TSV block consumed by the summary.
#   stage.<name> <seconds>          one row per pipeline stage
#   clip.<n>    <seconds> <status>  observed render wall time per clip
#   clips_ready / clips_failed      counts
#   total_wall  <seconds>           upload POST start -> terminal status
one_run() {
  local upload_resp project_id status begin now tsv prev
  upload_resp="$(curl -sf -X POST "$HOST/api/projects" -F "file=@${VIDEO}")" \
    || { echo "upload failed" >&2; return 1; }
  project_id="$(jq -r '.project.id // .id // empty' <<<"$upload_resp")"
  [[ -n "$project_id" ]] || { echo "no project id in upload response" >&2; return 1; }
  echo "project: $project_id" >&2

  begin="$(date +%s)"
  prev=""
  while :; do
    sleep "$POLL"
    now="$(date +%s)"
    tsv="$(curl -sf --max-time 10 "$HOST/api/projects/$project_id")" || continue
    status="$(jq -r '.project.status // "unknown"' <<<"$tsv")"
    # Record every clip status transition: "epoch clip_id status".
    jq -r '.clips[]? | "\(.id)\t\(.status)"' <<<"$tsv" | while IFS=$'\t' read -r cid cst; do
      printf '%s\t%s\t%s\n' "$now" "$cid" "$cst" >> "$TRANSITIONS"
    done
    if [[ "$status" =~ ^(complete|failed|cancelled)$ ]]; then
      jq -r '.project.error // empty' <<<"$tsv" >&2 || true
      echo "$tsv" > "$VIEW_JSON"
      break
    fi
  done

  # Stage wall times come straight from persisted started/completed stamps.
  jq -r 'def epoch: sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601;
         .project.stages[] | select(.started_at and .completed_at)
         | "stage.\(.name)\t\(((.completed_at | epoch) - (.started_at | epoch)) | . * 10 | round / 10)"' \
    "$VIEW_JSON"
  echo "clips_ready	$(jq '[.clips[]? | select(.status == "ready")] | length' "$VIEW_JSON")"
  echo "clips_failed	$(jq '[.clips[]? | select(.status == "failed")] | length' "$VIEW_JSON")"
  echo "status	$status"
  echo "total_wall	$(( $(date +%s) - begin ))"

  # Per-clip render spans from the transition log: first "rendering" to
  # first terminal state; clips never observed mid-render fall back to the
  # previous terminal's timestamp.
  python3 - "$TRANSITIONS" "$VIEW_JSON" <<'PY'
import json, sys

transitions = {}
for line in open(sys.argv[1]):
    epoch, cid, cst = line.rstrip("\n").split("\t")
    transitions.setdefault(cid, []).append((int(epoch), cst))
view = json.load(open(sys.argv[2]))
prev_done = None
for i, clip in enumerate(view.get("clips") or []):
    evs = transitions.get(clip["id"], [])
    first_seen = {}
    for epoch, cst in evs:
        first_seen.setdefault(cst, epoch)
    start = first_seen.get("rendering") or prev_done
    done = first_seen.get("ready") or first_seen.get("failed")
    secs = done - start if (start and done) else None
    prev_done = done or prev_done
    print(f"clip.{i + 1}\t{secs if secs is not None else ''}\t{clip['status']}\t{clip.get('duration_ms', 0) // 1000}s")
PY
}

echo "== source: $VIDEO"
medians=()
for run in $(seq 1 "$RUNS"); do
  TRANSITIONS="$(mktemp -t cf-time-transitions)"
  VIEW_JSON="$(mktemp -t cf-time-view)"
  : > "$TRANSITIONS"
  echo "── run $run/$RUNS"
  one_run | tee "run-${run}.tsv"
  medians+=("run-${run}.tsv")
done

(( RUNS > 1 )) || exit 0
echo "── medians across $RUNS runs"
python3 - "${medians[@]}" <<'PY'
import statistics, sys

stage_rows, clip_rows, totals = {}, {}, []
for path in sys.argv[1:]:
    for line in open(path):
        parts = line.rstrip("\n").split("\t")
        if line.startswith("stage."):
            stage_rows.setdefault(parts[0], []).append(float(parts[1]))
        elif line.startswith("clip.") and len(parts) >= 3 and parts[1]:
            clip_rows.setdefault(parts[0], []).append(float(parts[1]))
        elif line.startswith("total_wall"):
            totals.append(float(parts[1]))
for name in sorted(stage_rows):
    print(f"{name}\t{statistics.median(stage_rows[name])}")
for name in sorted(clip_rows):
    print(f"{name}\t{statistics.median(clip_rows[name])}")
if totals:
    print(f"total_wall\t{statistics.median(totals)}")
PY
rm -f run-*.tsv
