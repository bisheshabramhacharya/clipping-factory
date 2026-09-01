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
    --manifest|--run-dir|--baseline|--thresholds|--poll-seconds|--timeout-seconds)
      (($# >= 2)) || { echo "$1 requires a value" >&2; exit 1; }
      option="$1"; value="$2"; shift 2
      case "$option" in
        --manifest) MANIFEST="$value" ;;
        --run-dir) RUN_DIR="$value" ;;
        --baseline) BASELINE="$value" ;;
        --thresholds) THRESHOLDS="$value" ;;
        --poll-seconds) POLL="$value" ;;
        --timeout-seconds) TIMEOUT="$value" ;;
      esac
      ;;
    --resume) RESUME=true; shift ;;
    --enforce) ENFORCE=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

# Validate values before curl can make any network request.
[[ "$POLL" =~ ^[1-9][0-9]*$ ]] || { echo "--poll-seconds must be a positive integer" >&2; exit 1; }
[[ "$TIMEOUT" =~ ^[1-9][0-9]*$ ]] || { echo "--timeout-seconds must be a positive integer" >&2; exit 1; }
[[ "$ENFORCE" != true || -n "$BASELINE" ]] || { echo "--enforce requires --baseline" >&2; exit 1; }
[[ "$RESUME" != true || -n "$RUN_DIR" ]] || { echo "--resume requires --run-dir" >&2; exit 1; }

for cmd in curl jq python3; do command -v "$cmd" >/dev/null || { echo "$cmd is required" >&2; exit 1; }; done
[[ -f "$MANIFEST" ]] || { echo "Missing $MANIFEST; copy evals/manifest.example.json and edit local paths." >&2; exit 1; }
MANIFEST="$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$MANIFEST")"
MANIFEST_DIR="$(dirname "$MANIFEST")"

validate_manifest() {
  python3 - "$1" <<'PY'
import json,re,sys
p=sys.argv[1]
try:
    data=json.load(open(p,encoding='utf-8'))
except (OSError,json.JSONDecodeError) as exc:
    raise SystemExit(f'invalid manifest: {exc}')
if data.get('schema_version') != 1: raise SystemExit('manifest schema_version must be 1')
items=data.get('sources')
if not isinstance(items,list) or not items: raise SystemExit('manifest sources must be a non-empty list')
seen=set()
for item in items:
    if not isinstance(item,dict): raise SystemExit('manifest source entries must be objects')
    sid=item.get('id'); path=item.get('path')
    if not isinstance(sid,str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,79}',sid): raise SystemExit(f'invalid source id: {sid!r}')
    if sid in seen: raise SystemExit(f'duplicate source id: {sid}')
    if not isinstance(path,str) or not path: raise SystemExit(f'missing path for {sid}')
    seen.add(sid)
PY
}
validate_manifest "$MANIFEST"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RUN_DIR:-$ROOT/results/$RUN_ID}"
PROCESS_MANIFEST="$RUN_DIR/manifest.json"
if [[ "$RESUME" == true ]]; then
  [[ -f "$RUN_DIR/metadata.json" ]] || { echo "Resume requires valid $RUN_DIR/metadata.json" >&2; exit 1; }
  [[ -f "$PROCESS_MANIFEST" ]] || { echo "Resume requires snapshotted $PROCESS_MANIFEST" >&2; exit 1; }
  validate_manifest "$PROCESS_MANIFEST"
  RUN_ID="$(python3 - "$MANIFEST" "$PROCESS_MANIFEST" "$RUN_DIR/metadata.json" <<'PY'
import hashlib,json,re,sys
supplied,snapshot,metadata_path=sys.argv[1:]
try:
    metadata=json.load(open(metadata_path,encoding='utf-8'))
except (OSError,json.JSONDecodeError) as exc:
    raise SystemExit(f'invalid resume metadata: {exc}')
if metadata.get('schema_version') != 1 or not isinstance(metadata.get('run_id'),str) or not metadata['run_id']:
    raise SystemExit('invalid resume metadata schema or run_id')
recorded=metadata.get('manifest_sha256')
if not isinstance(recorded,str) or not re.fullmatch(r'[0-9a-f]{64}',recorded):
    raise SystemExit('invalid resume manifest_sha256')
def digest(path): return hashlib.sha256(open(path,'rb').read()).hexdigest()
supplied_hash=digest(supplied); snapshot_hash=digest(snapshot)
if supplied_hash != snapshot_hash or supplied_hash != recorded:
    raise SystemExit('resume manifest hash mismatch between supplied manifest, snapshot, and metadata')
print(metadata['run_id'])
PY
)"
else
  [[ ! -e "$RUN_DIR" ]] || { echo "$RUN_DIR already exists; use --resume or another --run-dir" >&2; exit 1; }
fi

# All local argument, manifest, and resume checks have completed.
SETUP="$(curl -sf "$HOST/api/setup")" || { echo "Studio not reachable at $HOST; run cargo run --release." >&2; exit 1; }
DATA_DIR="$(jq -r '.data_dir // empty' <<<"$SETUP")"
mkdir -p "$RUN_DIR/sources"

if [[ "$RESUME" != true ]]; then
  cp "$MANIFEST" "$PROCESS_MANIFEST"
  cp "$ROOT/rubric.csv" "$RUN_DIR/rubric.csv" 2>/dev/null || true
  STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  COMMIT="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  BRANCH="$(git branch --show-current 2>/dev/null || echo unknown)"
  DIRTY=false
  [[ -z "$(git status --porcelain --untracked-files=normal 2>/dev/null)" ]] || DIRTY=true
  python3 - "$RUN_DIR/metadata.json" "$RUN_ID" "$STARTED" "$COMMIT" "$BRANCH" "$DIRTY" "$PROCESS_MANIFEST" <<'PY'
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
  jq '{ffmpeg,ffmpeg_ass,ffprobe,whisper_ok,model_ok,model_mb,face_model_ok,disk_free_gb}' <<<"$SETUP" > "$RUN_DIR/environment.json"
fi

SOURCE_COUNT="$(jq '.sources | length' "$PROCESS_MANIFEST")"
for ((i=0; i<SOURCE_COUNT; i++)); do
  ID="$(jq -r ".sources[$i].id" "$PROCESS_MANIFEST")"
  REL="$(jq -r ".sources[$i].path" "$PROCESS_MANIFEST")"
  CATEGORY="$(jq -r ".sources[$i].category // \"unspecified\"" "$PROCESS_MANIFEST")"
  # Relative source paths remain anchored to the supplied manifest directory,
  # while source entries are read from the immutable run snapshot.
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
  if [[ -n "$DATA_DIR" && -f "$DATA_DIR/projects/$PROJECT/candidates.json" ]]; then
    cp "$DATA_DIR/projects/$PROJECT/candidates.json" "$OUT/selection.json"
  fi
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
