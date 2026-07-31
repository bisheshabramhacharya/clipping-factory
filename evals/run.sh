#!/usr/bin/env bash
# Run a private local golden set through the real Clipping Factory HTTP path.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
HOST="http://127.0.0.1:4571"
MANIFEST="$ROOT/manifest.json"
RUN_DIR=""
BASELINE=""
THRESHOLDS="$ROOT/thresholds.json"
POLL=5
TIMEOUT=21600
RESUME=false
ENFORCE=false

usage() {
  cat <<'EOF'
Usage: bash evals/run.sh [options]
  --manifest PATH        Local manifest (default evals/manifest.json)
  --run-dir PATH         Result directory (default evals/results/<UTC id>)
  --baseline PATH        Baseline run directory or report.json
  --thresholds PATH      Gate policy JSON
  --poll-seconds N       Poll interval (default 5)
  --timeout-seconds N    Per-source timeout (default 21600)
  --resume               Skip completed sources and retry unfinished/failed ones
  --enforce              Exit 2 when baseline thresholds fail
EOF
}

while (($#)); do
  case "$1" in
    --manifest) MANIFEST="$2"; shift 2 ;;
    --run-dir) RUN_DIR="$2"; shift 2 ;;
    --baseline) BASELINE="$2"; shift 2 ;;
    --thresholds) THRESHOLDS="$2"; shift 2 ;;
    --poll-seconds) POLL="$2"; shift 2 ;;
    --timeout-seconds) TIMEOUT="$2"; shift 2 ;;
    --resume) RESUME=true; shift ;;
    --enforce) ENFORCE=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

for cmd in curl jq python3; do command -v "$cmd" >/dev/null || { echo "$cmd is required" >&2; exit 1; }; done
[[ -f "$MANIFEST" ]] || { echo "Missing $MANIFEST; copy evals/manifest.example.json and edit local paths." >&2; exit 1; }
curl -sf "$HOST/api/setup" >/dev/null || { echo "Studio not reachable at $HOST; run cargo run --release." >&2; exit 1; }

MANIFEST="$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$MANIFEST")"
MANIFEST_DIR="$(dirname "$MANIFEST")"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RUN_DIR:-$ROOT/results/$RUN_ID}"
if [[ -e "$RUN_DIR" && "$RESUME" != true ]]; then echo "$RUN_DIR already exists; use --resume or another --run-dir" >&2; exit 1; fi
mkdir -p "$RUN_DIR/sources"

python3 - "$MANIFEST" <<'PY'
import json,re,sys
p=sys.argv[1]; data=json.load(open(p,encoding='utf-8'))
if data.get('schema_version') != 1: raise SystemExit('manifest schema_version must be 1')
items=data.get('sources')
if not isinstance(items,list) or not items: raise SystemExit('manifest sources must be a non-empty list')
seen=set()
for item in items:
    sid=item.get('id'); path=item.get('path')
    if not isinstance(sid,str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,79}',sid): raise SystemExit(f'invalid source id: {sid!r}')
    if sid in seen: raise SystemExit(f'duplicate source id: {sid}')
    if not isinstance(path,str) or not path: raise SystemExit(f'missing path for {sid}')
    seen.add(sid)
PY

if [[ "$RESUME" == true && -f "$RUN_DIR/metadata.json" ]]; then
  RUN_ID="$(jq -r '.run_id' "$RUN_DIR/metadata.json")"
else
  cp "$MANIFEST" "$RUN_DIR/manifest.json"
  cp "$ROOT/rubric.csv" "$RUN_DIR/rubric.csv" 2>/dev/null || true
  STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  BRANCH="$(git branch --show-current 2>/dev/null || echo unknown)"
  DIRTY=false; git diff --quiet --ignore-submodules HEAD 2>/dev/null || DIRTY=true
  python3 - "$RUN_DIR/metadata.json" "$RUN_ID" "$STARTED" "$COMMIT" "$BRANCH" "$DIRTY" "$MANIFEST" <<'PY'
import hashlib,json,platform,sys
path,run_id,started,commit,branch,dirty,manifest=sys.argv[1:]
data={'schema_version':1,'run_id':run_id,'started_at':started,'git_commit':commit,'git_branch':branch,'git_dirty':dirty=='true','platform':platform.platform(),'python':platform.python_version(),'manifest_sha256':hashlib.sha256(open(manifest,'rb').read()).hexdigest()}
json.dump(data,open(path,'w',encoding='utf-8'),indent=2,sort_keys=True); open(path,'a').write('\n')
PY
  python3 - "$RUN_DIR/tools.json" <<'PY'
import json,subprocess,sys
def version(*cmd):
    try: return subprocess.run(cmd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=False).stdout.splitlines()[0]
    except (OSError,IndexError): return 'unavailable'
json.dump({'rustc':version('rustc','--version'),'cargo':version('cargo','--version'),'ffmpeg':version('ffmpeg','-version'),'ffprobe':version('ffprobe','-version'),'jq':version('jq','--version')},open(sys.argv[1],'w'),indent=2,sort_keys=True); open(sys.argv[1],'a').write('\n')
PY
  # Store only booleans/numbers from setup. Paths and transcript/provider secrets are excluded.
  curl -sf "$HOST/api/setup" | jq '{ffmpeg,ffmpeg_ass,ffprobe,whisper_ok,model_ok,model_mb,face_model_ok,disk_free_gb}' > "$RUN_DIR/environment.json"
fi

SOURCE_COUNT="$(jq '.sources | length' "$MANIFEST")"
for ((i=0; i<SOURCE_COUNT; i++)); do
  ID="$(jq -r ".sources[$i].id" "$MANIFEST")"
  REL="$(jq -r ".sources[$i].path" "$MANIFEST")"
  CATEGORY="$(jq -r ".sources[$i].category // \"unspecified\"" "$MANIFEST")"
  SRC="$REL"; [[ "$SRC" = /* ]] || SRC="$MANIFEST_DIR/$SRC"
  OUT="$RUN_DIR/sources/$ID"; mkdir -p "$OUT"
  if [[ "$RESUME" == true && -f "$OUT/result.json" ]] && jq -e '.status == "complete"' "$OUT/result.json" >/dev/null; then echo "↷ $ID already complete"; continue; fi
  echo "── $ID"
  if [[ ! -f "$SRC" ]]; then
    jq -n --arg id "$ID" --arg cat "$CATEGORY" --arg error "source file not found" '{schema_version:1,source_id:$id,category:$cat,status:"failed",error:$error}' > "$OUT/result.json"
    continue
  fi
  BEGIN="$(date +%s)"
  UPLOAD="$OUT/upload-response.json"
  if ! curl -sf -X POST "$HOST/api/projects" -F "file=@$SRC" > "$UPLOAD"; then
    jq -n --arg id "$ID" --arg cat "$CATEGORY" --arg error "upload failed" '{schema_version:1,source_id:$id,category:$cat,status:"failed",error:$error}' > "$OUT/result.json"; continue
  fi
  PROJECT="$(jq -r '.project.id // .id // empty' "$UPLOAD")"
  if [[ -z "$PROJECT" ]]; then jq -n --arg id "$ID" --arg cat "$CATEGORY" --arg error "upload returned no project id" '{schema_version:1,source_id:$id,category:$cat,status:"failed",error:$error}' > "$OUT/result.json"; continue; fi
  FAILS=0; FINAL="unknown"; ERROR=""
  while :; do
    sleep "$POLL"
    if curl -sf "$HOST/api/projects/$PROJECT" > "$OUT/view.tmp"; then
      mv "$OUT/view.tmp" "$OUT/view.json"; FAILS=0
      FINAL="$(jq -r '.project.status // "unknown"' "$OUT/view.json")"
      [[ "$FINAL" =~ ^(complete|failed|cancelled)$ ]] && break
    else
      FAILS=$((FAILS+1)); [[ $FAILS -ge 3 ]] && { FINAL=failed; ERROR="three consecutive project polling failures"; break; }
    fi
    if (( $(date +%s) - BEGIN > TIMEOUT )); then FINAL=failed; ERROR="source timed out after ${TIMEOUT}s"; break; fi
  done
  if [[ -z "$ERROR" && "$FINAL" == failed ]]; then ERROR="$(jq -r '.project.error // .error // "project failed"' "$OUT/view.json" 2>/dev/null || echo project failed)"; fi
  DURATION="$(( $(date +%s) - BEGIN ))"
  jq -n --arg id "$ID" --arg cat "$CATEGORY" --arg status "$FINAL" --arg project "$PROJECT" --arg error "$ERROR" --argjson seconds "$DURATION" '{schema_version:1,source_id:$id,category:$cat,status:$status,project_id:$project,duration_seconds:$seconds,error:(if $error=="" then null else $error end)}' > "$OUT/result.json"
done

python3 - "$RUN_DIR/metadata.json" <<'PY'
import json,sys,datetime
p=sys.argv[1]; data=json.load(open(p)); data['completed_at']=datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace('+00:00','Z'); json.dump(data,open(p,'w'),indent=2,sort_keys=True); open(p,'a').write('\n')
PY
ARGS=("$ROOT/report.py" "$RUN_DIR" --thresholds "$THRESHOLDS")
if [[ -n "$BASELINE" ]]; then ARGS+=(--baseline "$BASELINE"); fi
if [[ "$ENFORCE" == true ]]; then ARGS+=(--enforce); fi
python3 "${ARGS[@]}"
