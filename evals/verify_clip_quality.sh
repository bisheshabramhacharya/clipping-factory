#!/usr/bin/env bash
# Verify clip quality end-to-end on the canonical test asset (issue #48).
#
# Renders a source through the real HTTP pipeline in an isolated studio (its
# own CF_DATA_DIR/CF_OUTPUT_DIR under the run dir, on its own port), then
# checks the rendered clips against the quality contract from spec #43:
#
#   locked crop   FaceCrop layouts carry one keyframe at t=0 AND the measured
#                 crop x stays constant across frames sampled through the clip
#   honest size   ffprobe output dims equal the native 9:16 crop window —
#                 downscale-only, capped at 1080x1920, never stretched up
#   encoder       the H.264 bitstream carries the libx264 SEI (no VideoToolbox)
#   first frames  the first frame of every clip is extracted into evidence/
#                 for visual review (face-first openings)
#   captions      a mid-clip frame is extracted alongside (caption scale)
#   health stack  cargo check / clippy / fmt / test, bash -n evals/run.sh
#
# Artifacts land in <run-dir>/report/ (default: evals/results/verify-<UTC>,
# which is gitignored). Exit 0 when every check passes, 1 otherwise.
#
# Usage: bash evals/verify_clip_quality.sh [options]
#   --source PATH        source video (default: canonical test asset)
#   --run-dir PATH       work + report directory
#   --port N             studio port (default 4573; must be free)
#   --timeout-seconds N  pipeline timeout (default 5400)
#   --poll-seconds N     status poll interval (default 5)
#   --reuse PROJECT_ID   skip build/render; re-check a project already inside
#                        --run-dir (requires a run dir made by this script)
#   --skip-health        skip the cargo health stack
#   --keep-server        leave the spawned studio running (debugging)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$HOME/Downloads/clipping-test-EEefUdNCs94.mp4"
RUN_DIR=""
PORT=4573
POLL=5
TIMEOUT=5400
REUSE=""
SKIP_HEALTH=false
KEEP_SERVER=false

usage() { sed -n '2,30p' "$0"; }

while (($#)); do
  case "$1" in
    --source|--run-dir|--port|--timeout-seconds|--poll-seconds|--reuse)
      (($# >= 2)) || { echo "$1 requires a value" >&2; exit 1; }
      option="$1"; value="$2"; shift 2
      case "$option" in
        --source) SOURCE="$value" ;;
        --run-dir) RUN_DIR="$value" ;;
        --port) PORT="$value" ;;
        --timeout-seconds) TIMEOUT="$value" ;;
        --poll-seconds) POLL="$value" ;;
        --reuse) REUSE="$value" ;;
      esac
      ;;
    --skip-health) SKIP_HEALTH=true; shift ;;
    --keep-server) KEEP_SERVER=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

[[ "$POLL" =~ ^[1-9][0-9]*$ ]] || { echo "--poll-seconds must be a positive integer" >&2; exit 1; }
[[ "$TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { echo "--timeout-seconds must be a positive integer" >&2; exit 1; }
[[ "$PORT" =~ ^[0-9]+$ ]] || { echo "--port must be an integer" >&2; exit 1; }
for cmd in curl jq python3 cargo ffmpeg ffprobe; do
  command -v "$cmd" >/dev/null || { echo "$cmd is required" >&2; exit 1; }
done
[[ -f "$SOURCE" ]] || { echo "Source not found: $SOURCE" >&2; exit 1; }
SOURCE="$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$SOURCE")"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RUN_DIR:-$ROOT/evals/results/verify-$RUN_ID}"
REPORT="$RUN_DIR/report"
EVIDENCE="$REPORT/evidence"
DATA_DIR="$RUN_DIR/data"
OUTPUT_DIR="$RUN_DIR/output"
mkdir -p "$REPORT" "$EVIDENCE"

# Resolve ffmpeg/ffprobe the same way src/config.rs does so the checks run
# against the same binaries the pipeline used.
FFMPEG="${CF_FFMPEG:-}"
FFPROBE="${CF_FFPROBE:-}"
if [[ -z "$FFMPEG" ]]; then
  if [[ -x /opt/homebrew/opt/ffmpeg-full/bin/ffmpeg ]]; then
    FFMPEG=/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg
  else
    FFMPEG=ffmpeg
  fi
fi
if [[ -z "$FFPROBE" ]]; then
  if [[ -x /opt/homebrew/opt/ffmpeg-full/bin/ffprobe ]]; then
    FFPROBE=/opt/homebrew/opt/ffmpeg-full/bin/ffprobe
  else
    FFPROBE=ffprobe
  fi
fi

SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" && "$KEEP_SERVER" != true ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

HOST="http://127.0.0.1:$PORT"
PROJECT="$REUSE"

if [[ -z "$REUSE" ]]; then
  if curl -sf --max-time 2 "$HOST/api/setup" >/dev/null 2>&1; then
    echo "Port $PORT already serves a studio; pick another --port." >&2
    exit 1
  fi

  echo "── build: cargo build --release"
  (cd "$ROOT" && cargo build --release)

  # An isolated CF_DATA_DIR cannot see ~/.clipping-factory/models, so point the
  # whisper model explicitly at the usual local location when the caller did
  # not already choose one.
  if [[ -z "${CF_WHISPER_MODEL:-}" ]]; then
    for m in "$HOME/.clipping-factory/models/ggml-small.en.bin" \
             "$HOME/.clipping-factory/models/ggml-base.en.bin"; do
      if [[ -f "$m" ]]; then export CF_WHISPER_MODEL="$m"; break; fi
    done
  fi

  echo "── studio: http://127.0.0.1:$PORT (data: $DATA_DIR)"
  mkdir -p "$DATA_DIR" "$OUTPUT_DIR"
  (
    cd "$ROOT"
    CF_NO_OPEN=1 CF_PORT="$PORT" CF_DATA_DIR="$DATA_DIR" CF_OUTPUT_DIR="$OUTPUT_DIR" \
      ./target/release/clipping-factory >"$RUN_DIR/studio.log" 2>&1
  ) &
  SERVER_PID=$!

  for _ in $(seq 1 60); do
    curl -sf --max-time 2 "$HOST/api/setup" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -sf "$HOST/api/setup" > "$REPORT/setup.json" \
    || { echo "Studio did not come up; see $RUN_DIR/studio.log" >&2; exit 1; }
  jq '{ffmpeg,ffmpeg_ass,ffprobe,whisper_ok,model_ok,model_mb,face_model_ok,disk_free_gb}' \
    "$REPORT/setup.json" > "$REPORT/environment.json"

  echo "── upload: $(basename "$SOURCE")"
  curl -sf -X POST "$HOST/api/projects" -F "file=@$SOURCE" > "$REPORT/upload-response.json"
  PROJECT="$(jq -r '.project.id // .id // empty' "$REPORT/upload-response.json")"
  [[ -n "$PROJECT" ]] || { echo "Upload returned no project id" >&2; exit 1; }
  echo "   project: $PROJECT"

  BEGIN="$(date +%s)"
  LAST=""
  while :; do
    sleep "$POLL"
    if curl -sf "$HOST/api/projects/$PROJECT" > "$RUN_DIR/view.tmp"; then
      mv "$RUN_DIR/view.tmp" "$REPORT/view.json"
      STATUS="$(jq -r '.project.status // "unknown"' "$REPORT/view.json")"
      STAGE="$(jq -r '[.project.stages[] | select(.completed_at == null)] | .[0] // empty | .name // ""' "$REPORT/view.json" 2>/dev/null || true)"
      if [[ "$STATUS/$STAGE" != "$LAST" ]]; then
        echo "   status: $STATUS ${STAGE:+($STAGE)}"
        LAST="$STATUS/$STAGE"
      fi
      [[ "$STATUS" =~ ^(complete|failed|cancelled)$ ]] && break
    fi
    if (( $(date +%s) - BEGIN > TIMEOUT )); then
      echo "Pipeline timed out after ${TIMEOUT}s" >&2
      STATUS="failed"
      break
    fi
  done
  if [[ "$STATUS" != "complete" ]]; then
    jq -r '.project.error // "project did not complete"' "$REPORT/view.json" >&2 || true
  fi
fi

MANIFEST=""
if [[ -n "$PROJECT" && -f "$DATA_DIR/projects/$PROJECT/render-manifest.json" ]]; then
  MANIFEST="$DATA_DIR/projects/$PROJECT/render-manifest.json"
  cp "$MANIFEST" "$REPORT/render-manifest.json"
fi

# --------------------------------------------------------------------------
# Clip checks: ffprobe dims/codec, libx264 SEI, locked-crop (manifest + measured
# crop x over sampled frames), first-frame and caption-frame grabs.
# --------------------------------------------------------------------------
CHECKS_JSON="$REPORT/checks.json"
if [[ -n "$MANIFEST" ]]; then
python3 - "$MANIFEST" "$DATA_DIR/projects/$PROJECT/clips" "$SOURCE" "$EVIDENCE" "$CHECKS_JSON" "$FFMPEG" "$FFPROBE" <<'PY'
import json, os, re, subprocess, sys, tempfile

manifest_path, clips_dir, source, evidence_dir, out_json, FFMPEG, FFPROBE = sys.argv[1:]
manifest = json.load(open(manifest_path))

def run(cmd):
    return subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

def probe(path):
    p = run([FFPROBE, "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=codec_name,width,height,avg_frame_rate,duration:format=duration",
             "-of", "json", path])
    return json.loads(p.stdout or "{}")

def has_x264_sei(path):
    # libx264 stamps an encoder-info SEI into the first access unit;
    # h264_videotoolbox does not. Dump the opening bitstream and look for it.
    p = run([FFMPEG, "-v", "error", "-i", path, "-frames:v", "4",
             "-c", "copy", "-bsf:v", "h264_mp4toannexb", "-f", "h264", "-"])
    return b"x264" in p.stdout

def grab(video, t_s, out_png):
    run([FFMPEG, "-y", "-v", "error", "-ss", f"{t_s:.3f}", "-i", video,
         "-frames:v", "1", out_png])

def scaled_source_frame(t_s, w, h, out_png):
    run([FFMPEG, "-y", "-v", "error", "-ss", f"{t_s:.3f}", "-i", source,
         "-vf", f"scale={w}:{h}", "-frames:v", "1", out_png])

PSNR_RE = re.compile(rb"average:(\d+\.\d+|inf)")
def psnr_at(src_png, clip_png, w, h, x):
    # Compare a caption-free top band: captions occupy the lower-middle of the
    # canvas, so matching the top 40% isolates the crop window position.
    fc = (f"[0:v]crop={w}:{h}:{x}:0[s];"
          f"[1:v]crop={w}:{h}:0:0[c];[s][c]psnr")
    p = run([FFMPEG, "-i", src_png, "-i", clip_png,
             "-filter_complex", fc, "-f", "null", "-"])
    m = PSNR_RE.search(p.stderr)
    return float("inf") if (m and m.group(1) == b"inf") else (float(m.group(1)) if m else 0.0)

def measure_crop_x(clip_path, start_ms, dur_s, out_w, out_h, src_w, src_h, tmp):
    """Best-match crop x at sampled clip times via template matching against
    the source scaled to output height (the same math render.rs uses)."""
    scaled_h = out_h
    scaled_w = int(round(src_w * scaled_h / src_h / 2)) * 2
    reach = scaled_w - out_w
    if reach <= 0:
        return [], "clip as wide as scaled source; nothing to sweep"
    band_h = int(out_h * 0.40) // 2 * 2
    if band_h < 64:
        band_h = out_h // 2 * 2
    coarse = max(16, int(round(reach / 40 / 2)) * 2)
    times = [max(0.4, dur_s * f) for f in (0.05, 0.275, 0.5, 0.725)]
    times.append(max(0.4, min(dur_s - 0.4, dur_s * 0.95)))
    samples = []
    for t in times:
        cpng = os.path.join(tmp, f"c{len(samples)}.png")
        spng = os.path.join(tmp, f"s{len(samples)}.png")
        grab(clip_path, t, cpng)
        scaled_source_frame(start_ms / 1000.0 + t, scaled_w, scaled_h, spng)
        if not (os.path.isfile(cpng) and os.path.isfile(spng)):
            samples.append({"t_s": round(t, 2), "error": "frame extraction failed"})
            continue
        best_x, best_p = 0, -1.0
        for x in range(0, reach + 1, coarse):
            p = psnr_at(spng, cpng, out_w, band_h, x)
            if p > best_p:
                best_x, best_p = x, p
        lo = max(0, best_x - coarse)
        hi = min(reach, best_x + coarse)
        for x in range(lo, hi + 1, 6):
            p = psnr_at(spng, cpng, out_w, band_h, x)
            if p > best_p:
                best_x, best_p = x, p
        samples.append({"t_s": round(t, 2), "x": best_x,
                        "psnr_db": (None if best_p == float("inf") else round(best_p, 2))})
    return samples, None

def even(x):
    return int(round(x / 2)) * 2

def native_window(sw, sh):
    # Largest 9:16 window inside the source at native scale (downscale cap).
    if sw / sh > 9 / 16:
        w, h = even(sh * 9 / 16), sh
    else:
        w, h = even(sw), min(sh - sh % 2, even(sw * 16 / 9))
    if h > 1920 or w > 1080:
        w, h = 1080, 1920
    return w, h

src_probe = probe(source)
sv = (src_probe.get("streams") or [{}])[0]
src_w, src_h = int(sv.get("width") or 0), int(sv.get("height") or 0)
if not src_w or not src_h:
    json.dump({"clips": [], "clips_ok": False,
               "error": "could not probe source dimensions"}, open(out_json, "w"))
    sys.exit(0)
exp_w, exp_h = native_window(src_w, src_h)

X_SPREAD_TOL = 16  # px — measurement noise is a few px; real pans move hundreds

report = {
    "source": {"path": source, "width": src_w, "height": src_h,
               "codec": sv.get("codec_name"), "fps": sv.get("avg_frame_rate")},
    "expected_window": {"w": exp_w, "h": exp_h},
    "x_spread_tolerance_px": X_SPREAD_TOL,
    "clips": [],
}
all_ok = True
for clip in manifest.get("clips", []):
    entry = {"filename": clip.get("filename"), "status": clip.get("status"),
             "layout": clip.get("layout", {}).get("mode"),
             "start_ms": clip.get("start_ms"), "end_ms": clip.get("end_ms")}
    keyframes = clip.get("layout", {}).get("keyframes") or []
    entry["keyframes"] = keyframes
    entry["manifest_locked"] = (
        entry["layout"] == "face_crop" and len(keyframes) == 1
        and keyframes[0].get("t_ms") == 0)
    path = os.path.join(clips_dir, clip.get("filename") or "")
    if clip.get("status") != "ready" or not os.path.isfile(path):
        entry["error"] = "clip not rendered"
        entry["checks_ok"] = False
        all_ok = False
        report["clips"].append(entry)
        continue

    pr = probe(path)
    v = (pr.get("streams") or [{}])[0]
    w, h = int(v.get("width") or 0), int(v.get("height") or 0)
    dur_s = float(v.get("duration") or (pr.get("format") or {}).get("duration") or 0)
    entry.update(codec=v.get("codec_name"), width=w, height=h,
                 duration_s=round(dur_s, 2),
                 x264_sei=has_x264_sei(path))
    entry["resolution_ok"] = (abs(w - exp_w) <= 2 and abs(h - exp_h) <= 2
                              and w <= 1080 and h <= 1920
                              and w <= src_w and h <= src_h)
    entry["encoder_ok"] = (v.get("codec_name") == "h264" and entry["x264_sei"])

    stem = os.path.splitext(clip.get("filename") or "clip")[0]
    first_png = os.path.join(evidence_dir, f"{stem}_first.png")
    cap_png = os.path.join(evidence_dir, f"{stem}_caption.png")
    grab(path, 0.0, first_png)
    grab(path, entry["duration_s"] * 0.4, cap_png)
    entry["first_frame"] = os.path.relpath(first_png, os.path.dirname(out_json))
    entry["caption_frame"] = os.path.relpath(cap_png, os.path.dirname(out_json))

    if entry["layout"] == "face_crop":
        with tempfile.TemporaryDirectory() as tmp:
            samples, note = measure_crop_x(path, clip.get("start_ms", 0),
                                           entry["duration_s"], w, h,
                                           src_w, src_h, tmp)
        entry["motion_samples"] = samples
        if note:
            entry["motion_note"] = note
        good = [s["x"] for s in samples if "x" in s]
        entry["x_spread"] = (max(good) - min(good)) if len(good) >= 2 else None
        entry["motion_free"] = (entry["x_spread"] is not None
                                and entry["x_spread"] <= X_SPREAD_TOL)
        entry["crop_ok"] = bool(entry["manifest_locked"] and entry["motion_free"])
    else:
        entry["crop_ok"] = None  # no crop window in blur_pad
    entry["checks_ok"] = bool(
        entry["resolution_ok"] and entry["encoder_ok"]
        and entry["crop_ok"] is not False)
    all_ok = all_ok and entry["checks_ok"]
    report["clips"].append(entry)

report["clips_ok"] = all_ok and bool(report["clips"])
json.dump(report, open(out_json, "w"), indent=2)
print(json.dumps({"clips": len(report["clips"]), "clips_ok": report["clips_ok"]}))
PY
else
  echo '{"clips": [], "clips_ok": false, "error": "no render-manifest.json"}' > "$CHECKS_JSON"
fi

# --------------------------------------------------------------------------
# Health stack (AGENTS.md).
# --------------------------------------------------------------------------
HEALTH_TSV="$REPORT/health.tsv"
: > "$HEALTH_TSV"
if [[ "$SKIP_HEALTH" == true ]]; then
  echo -e "skipped\t(— --skip-health)" >> "$HEALTH_TSV"
else
  echo "── health stack"
  while IFS= read -r cmd; do
    [[ -z "$cmd" ]] && continue
    if (cd "$ROOT" && eval "$cmd") > "$REPORT/health-$(echo "$cmd" | tr -cd 'a-z' | cut -c1-24).log" 2>&1; then
      echo -e "pass\t$cmd" >> "$HEALTH_TSV"
    else
      echo -e "FAIL\t$cmd" >> "$HEALTH_TSV"
    fi
  done <<'EOF'
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
cargo test
bash -n evals/run.sh
EOF
  cat "$HEALTH_TSV"
fi

# --------------------------------------------------------------------------
# Report.
# --------------------------------------------------------------------------
python3 - "$CHECKS_JSON" "$HEALTH_TSV" "$REPORT/verify-report.md" "$RUN_DIR" <<'PY'
import json, sys

checks_path, health_path, out_path, run_dir = sys.argv[1:]
data = json.load(open(checks_path))
health = [l.rstrip("\n").split("\t", 1) for l in open(health_path) if l.strip()]
health_ok = all(r[0] != "FAIL" for r in health) if health else False
clips = data.get("clips", [])
clips_ok = data.get("clips_ok", False)

L = []
L.append("# Clip quality verification — issue #48\n")
L.append(f"- run dir: `{run_dir}`")
s = data.get("source", {})
L.append(f"- source: `{s.get('path')}` ({s.get('width')}x{s.get('height')}, {s.get('codec')})")
ew = data.get("expected_window", {})
L.append(f"- native 9:16 window (downscale cap): {ew.get('w')}x{ew.get('h')}")
L.append(f"- clips in manifest: {len(clips)}\n")

L.append("| clip | layout | keyframes | out size | codec / x264 SEI | crop x samples | verdict |")
L.append("|---|---|---|---|---|---|---|")
for c in clips:
    kf = c.get("keyframes") or []
    kf_desc = f"{len(kf)} keys" if len(kf) != 1 else "1 key @t=0 (locked)"
    samples = ", ".join(
        f"x={m['x']}@{m['t_s']}s" if "x" in m else "err"
        for m in c.get("motion_samples", [])) or "—"
    if c.get("x_spread") is not None:
        samples += f" (spread {c['x_spread']}px)"
    verdict = ("PASS" if c.get("checks_ok") else "FAIL")
    fails = []
    if c.get("layout") == "face_crop" and c.get("manifest_locked") is False:
        fails.append("crop not locked")
    if c.get("motion_free") is False: fails.append("crop moves")
    if c.get("resolution_ok") is False: fails.append("wrong size")
    if c.get("encoder_ok") is False: fails.append("not libx264")
    if fails: verdict += ": " + ", ".join(fails)
    L.append("| {} | {} | {} | {}x{} | {} / {} | {} | {} |".format(
        c.get("filename"), c.get("layout"), kf_desc,
        c.get("width", "?"), c.get("height", "?"),
        c.get("codec", "?"), "x264" if c.get("x264_sei") else "no SEI",
        samples, verdict))
L.append("")
L.append("## Health stack\n")
L.append("| result | command |")
L.append("|---|---|")
for r in health:
    L.append(f"| {r[0]} | `{r[1]}` |")
L.append("")
L.append(f"## Verdict: {'PASS' if (clips_ok and health_ok) else 'FAIL'}\n")
L.append("Frame grabs for visual review (face-first openings, caption scale) are in `evidence/`.")

open(out_path, "w").write("\n".join(L) + "\n")
print("\n".join(L))
sys.exit(0 if (clips_ok and health_ok) else 1)
PY
