#!/usr/bin/env bash
# Run every MP4 in evals/sources/ through a running Clipping Factory studio
# and collect project views, provenance, and a comparable summary.
#
# Usage:
#   bash evals/run.sh [host]
#   bash evals/run.sh --fixtures
#
# The fixture mode validates the copyright-free synthetic controls and, unless
# CF_EVAL_RUN_CARGO=0, runs their focused Rust tests without requiring Studio.
set -euo pipefail

HOST="${CF_EVAL_HOST:-http://localhost:4571}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$ROOT/.." && pwd)"
SOURCES="$ROOT/sources"
RESULTS_ROOT="${CF_EVAL_RESULTS_DIR:-$ROOT/results}"
TIMEOUT_SECONDS="${CF_EVAL_TIMEOUT_SECONDS:-7200}"
POLL_SECONDS="${CF_EVAL_POLL_SECONDS:-5}"
BASELINE="${CF_EVAL_BASELINE:-}"
MODE="real_media"
TEMP_TARGET_DIR=""
readonly CURL_CONNECT_TIMEOUT_SECONDS=10
readonly CURL_CANCEL_TIMEOUT_SECONDS=10

usage() {
  cat <<'EOF'
Usage: bash evals/run.sh [host] [--timeout SECONDS] [--baseline SUMMARY]
       bash evals/run.sh --fixtures [--baseline SUMMARY]

Environment overrides:
  CF_EVAL_RESULTS_DIR       output root (default evals/results)
  CF_EVAL_TIMEOUT_SECONDS   per-source wall-clock timeout (default 7200)
  CF_EVAL_POLL_SECONDS      project poll interval (default 5)
  CF_EVAL_BASELINE          previous summary.json to compare
  CF_EVAL_RUN_CARGO=0       fixture mode: validate manifests without cargo
  CF_EVAL_TARGET_DIR        fixture mode: external cargo target directory
EOF
}

die() {
  echo "evals/run.sh: $*" >&2
  exit 1
}

cleanup_temp_target() {
  if [[ -n "$TEMP_TARGET_DIR" && -d "$TEMP_TARGET_DIR" ]]; then
    rm -rf -- "$TEMP_TARGET_DIR"
  fi
  TEMP_TARGET_DIR=""
}
trap cleanup_temp_target EXIT

seconds_remaining() {
  local deadline="$1"
  local now
  now="$(date +%s)"
  ((now < deadline)) || return 1
  echo $((deadline - now))
}

curl_bounded() {
  local max_time="$1"
  shift
  local connect_time="$CURL_CONNECT_TIMEOUT_SECONDS"
  if ((connect_time > max_time)); then
    connect_time="$max_time"
  fi
  curl --connect-timeout "$connect_time" --max-time "$max_time" "$@"
}

cancel_project() {
  local id="$1"
  curl_bounded "$CURL_CANCEL_TIMEOUT_SECONDS" -sf -X POST \
    "$HOST/api/projects/$id/cancel" >/dev/null 2>&1 || true
}

sha256_file() {
  local path="$1"
  if command -v shasum >/dev/null; then
    shasum -a 256 -- "$path" | awk '{print $1}'
  else
    sha256sum -- "$path" | awk '{print $1}'
  fi
}

while (($# > 0)); do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --fixtures|--synthetic)
      MODE="synthetic"
      shift
      ;;
    --timeout)
      (($# >= 2)) || die "--timeout requires seconds"
      TIMEOUT_SECONDS="$2"
      shift 2
      ;;
    --baseline)
      (($# >= 2)) || die "--baseline requires a summary.json path"
      BASELINE="$2"
      shift 2
      ;;
    http://*|https://*)
      HOST="$1"
      shift
      ;;
    *)
      die "unknown argument '$1' (use --help)"
      ;;
  esac
done

[[ "$TIMEOUT_SECONDS" =~ ^[1-9][0-9]*$ ]] || die "timeout must be a positive integer"
[[ "$POLL_SECONDS" =~ ^[1-9][0-9]*$ ]] || die "poll interval must be a positive integer"
command -v jq >/dev/null || die "jq is required (brew install jq)"
if ! command -v shasum >/dev/null && ! command -v sha256sum >/dev/null; then
  die "shasum or sha256sum is required for source fingerprints"
fi

shopt -s nullglob
mkdir -p "$RESULTS_ROOT"
RUN_DIR="$RESULTS_ROOT/$(date +%Y%m%d-%H%M%S)-$$"
mkdir -p "$RUN_DIR"

git_commit="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
git_branch="$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo unknown)"
git_status="$(git -C "$REPO_ROOT" status --short 2>/dev/null || true)"
git_diff_stat="$(git -C "$REPO_ROOT" diff --stat 2>/dev/null || true)"

write_provenance() {
  local app_setup="$1"
  jq -n \
    --arg commit "$git_commit" \
    --arg branch "$git_branch" \
    --arg status "$git_status" \
    --arg diff_stat "$git_diff_stat" \
    --arg repo_root "$REPO_ROOT" \
    --arg host "$HOST" \
    --arg mode "$MODE" \
    --arg timeout "$TIMEOUT_SECONDS" \
    --arg poll "$POLL_SECONDS" \
    --arg baseline "$BASELINE" \
    --arg results_root "$RESULTS_ROOT" \
    --argjson app_setup "$app_setup" \
    '{
      commit: $commit,
      branch: $branch,
      worktree_status: $status,
      diff_stat: $diff_stat,
      repo_root: $repo_root,
      config: {
        host: $host,
        mode: $mode,
        timeout_seconds: ($timeout | tonumber),
        poll_seconds: ($poll | tonumber),
        baseline: (if $baseline == "" then null else $baseline end),
        results_root: $results_root,
        run_cargo_fixture_tests: (env.CF_EVAL_RUN_CARGO // "1")
      },
      app_setup: $app_setup
    }' > "$RUN_DIR/provenance.json"
}

write_delta() {
  local summary_file="$1"
  local baseline_path="$BASELINE"
  local baseline_json="null"
  local candidate candidate_mode

  if [[ -n "$baseline_path" ]]; then
    [[ -f "$baseline_path" ]] || die "baseline summary not found: $baseline_path"
  else
    for candidate in "$RESULTS_ROOT"/*/summary.json; do
      [[ "$candidate" == "$summary_file" ]] && continue
      candidate_mode="$(jq -er '.mode // empty' "$candidate" 2>/dev/null || true)"
      [[ "$candidate_mode" == "$MODE" ]] || continue
      if [[ -z "$baseline_path" || "$candidate" > "$baseline_path" ]]; then
        baseline_path="$candidate"
      fi
    done
  fi

  if [[ -n "$baseline_path" ]]; then
    baseline_json="$(<"$baseline_path")"
    jq -e . >/dev/null <<<"$baseline_json" || die "baseline is not valid JSON: $baseline_path"
    candidate_mode="$(jq -r '.mode // empty' <<<"$baseline_json")"
    [[ "$candidate_mode" == "$MODE" ]] || \
      die "baseline mode '$candidate_mode' does not match current mode '$MODE': $baseline_path"
  fi

  local current_json
  current_json="$(<"$summary_file")"
  jq -n \
    --argjson current "$current_json" \
    --argjson baseline "$baseline_json" \
    --arg baseline_path "$baseline_path" \
    '{
      baseline: (if $baseline == null then null else {path: $baseline_path, summary: $baseline} end),
      current: $current,
      delta: (
        if $baseline == null then null
        elif $current.mode == "real_media" and $baseline.mode == "real_media" then {
          ready_clips: (($current.totals.ready_clips // 0) - ($baseline.totals.ready_clips // 0)),
          failed_clips: (($current.totals.failed_clips // 0) - ($baseline.totals.failed_clips // 0)),
          rejected_candidates: (($current.totals.rejected_candidates // 0) - ($baseline.totals.rejected_candidates // 0)),
          terminal_failures: (($current.totals.terminal_failures // 0) - ($baseline.totals.terminal_failures // 0))
        }
        elif $current.mode == "synthetic" and $baseline.mode == "synthetic" then {
          expected_accept: (($current.editorial_fixtures.expected_accept // 0) - ($baseline.editorial_fixtures.expected_accept // 0)),
          expected_reject: (($current.editorial_fixtures.expected_reject // 0) - ($baseline.editorial_fixtures.expected_reject // 0)),
          selector_test_changed: (($current.editorial_fixtures.selector_test // "") != ($baseline.editorial_fixtures.selector_test // "")),
          overlap_test_changed: (($current.overlap_fixture.test // "") != ($baseline.overlap_fixture.test // ""))
        }
        else {error: "baseline mode does not match current mode"}
        end
      )
    }' > "$RUN_DIR/delta.json"
}

run_synthetic() {
  local editorial="$ROOT/fixtures/editorial_cases.json"
  local overlap="$ROOT/fixtures/overlap_containment.json"
  [[ -f "$editorial" ]] || die "missing fixture manifest: $editorial"
  [[ -f "$overlap" ]] || die "missing fixture manifest: $overlap"

  jq -e '
    type == "array" and length > 0 and
    all(.[];
      (.name | type) == "string" and
      ((.expected == "accept") or (.expected == "reject")) and
      (.sentences | type) == "array" and (.sentences | length) > 0)
  ' "$editorial" >/dev/null || die "invalid editorial fixture manifest"
  jq -e '
    (.name | type) == "string" and
    (.higher_ranked.start_ms | type) == "number" and
    (.higher_ranked.end_ms | type) == "number" and
    (.lower_ranked.start_ms | type) == "number" and
    (.lower_ranked.end_ms | type) == "number" and
    (.expected | type) == "string"
  ' "$overlap" >/dev/null || die "invalid overlap fixture manifest"

  local total expected_accept expected_reject overlap_name overlap_expected
  total="$(jq 'length' "$editorial")"
  expected_accept="$(jq '[.[] | select(.expected == "accept")] | length' "$editorial")"
  expected_reject="$(jq '[.[] | select(.expected == "reject")] | length' "$editorial")"
  overlap_name="$(jq -r '.name' "$overlap")"
  overlap_expected="$(jq -r '.expected' "$overlap")"

  local selector_test="skipped" overlap_test="skipped"
  if [[ "${CF_EVAL_RUN_CARGO:-1}" == "1" ]]; then
    if command -v cargo >/dev/null; then
      local target_dir
      if [[ -n "${CF_EVAL_TARGET_DIR:-}" ]]; then
        target_dir="$CF_EVAL_TARGET_DIR"
      else
        TEMP_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/clipping-factory-evals-target.XXXXXX")"
        target_dir="$TEMP_TARGET_DIR"
      fi
      mkdir -p "$target_dir"
      if cargo test --locked --target-dir "$target_dir" \
        select::heuristic::tests::synthetic_editorial_fixtures_match_selector_expectations \
        -- --exact --nocapture > "$RUN_DIR/selector-test.log" 2>&1; then
        selector_test="passed"
      else
        selector_test="failed"
      fi
      if cargo test --locked --target-dir "$target_dir" \
        validate::tests::a_candidate_containing_a_higher_ranked_clip_is_rejected_as_overlap \
        -- --exact --nocapture >> "$RUN_DIR/selector-test.log" 2>&1; then
        overlap_test="passed"
      else
        overlap_test="failed"
      fi
      cleanup_temp_target
    else
      selector_test="cargo-not-found"
      overlap_test="cargo-not-found"
    fi
  fi

  local app_setup='null'
  write_provenance "$app_setup"
  jq -n \
    --arg mode "$MODE" \
    --argjson total "$total" \
    --argjson expected_accept "$expected_accept" \
    --argjson expected_reject "$expected_reject" \
    --arg selector_test "$selector_test" \
    --arg overlap_name "$overlap_name" \
    --arg overlap_expected "$overlap_expected" \
    --arg overlap_test "$overlap_test" \
    '{
      mode: $mode,
      editorial_fixtures: {
        total: $total,
        expected_accept: $expected_accept,
        expected_reject: $expected_reject,
        selector_test: $selector_test
      },
      overlap_fixture: {
        name: $overlap_name,
        expected: $overlap_expected,
        test: $overlap_test
      }
    }' > "$RUN_DIR/summary.json"
  write_delta "$RUN_DIR/summary.json"
  echo "Synthetic eval → $RUN_DIR"
  echo "  $total editorial fixtures ($expected_accept expected accept, $expected_reject expected reject)"
  echo "  selector test: $selector_test; overlap test: $overlap_test"
  echo "  baseline/delta: $RUN_DIR/delta.json"
  if [[ "$selector_test" != "passed" || "$overlap_test" != "passed" ]]; then
    return 1
  fi
  return 0
}

if [[ "$MODE" == "synthetic" ]]; then
  run_synthetic
  exit $?
fi

setup_json=""
if ! setup_json="$(curl_bounded "$TIMEOUT_SECONDS" -sf "$HOST/api/setup")"; then
  die "Studio not reachable at $HOST — start it with: cargo run --release"
fi
jq -e . >/dev/null <<<"$setup_json" || die "Studio returned invalid JSON from /api/setup"
write_provenance "$setup_json"

mp4s=("$SOURCES"/*.mp4 "$SOURCES"/*.m4v)
[[ ${#mp4s[@]} -gt 0 ]] || die "No MP4s in $SOURCES — add golden-set episodes first (see evals/README.md)"

echo "Eval run → $RUN_DIR (${#mp4s[@]} source(s))"
summary_sources='[]'
run_failed=0
source_index=0

for src in "${mp4s[@]}"; do
  source_index=$((source_index + 1))
  name="$(basename "$src")"
  echo ""
  echo "── $name"

  started_epoch="$(date +%s)"
  deadline=$((started_epoch + TIMEOUT_SECONDS))
  source_sha256="$(sha256_file "$src")"
  remaining=""
  if ! remaining="$(seconds_remaining "$deadline")"; then
    echo "   source fingerprinting exceeded the ${TIMEOUT_SECONDS}s deadline" >&2
    summary_sources="$(jq -c --arg source "$name" --arg sha256 "$source_sha256" '. + [{source: $source, source_sha256: $sha256, status: "upload_timeout", terminal: false, ready_clips: 0, failed_clips: 0, rejected_candidates: 0}]' <<<"$summary_sources")"
    run_failed=1
    continue
  fi

  project_json=""
  if ! project_json="$(curl_bounded "$remaining" -sf -X POST "$HOST/api/projects" -F "file=@$src")"; then
    if seconds_remaining "$deadline" >/dev/null; then
      status="upload_failed"
      echo "   upload failed" >&2
    else
      status="upload_timeout"
      echo "   upload exceeded the ${TIMEOUT_SECONDS}s deadline" >&2
    fi
    summary_sources="$(jq -c --arg source "$name" --arg sha256 "$source_sha256" --arg status "$status" '. + [{source: $source, source_sha256: $sha256, status: $status, terminal: false, ready_clips: 0, failed_clips: 0, rejected_candidates: 0}]' <<<"$summary_sources")"
    run_failed=1
    continue
  fi
  if ! id="$(jq -er '.project.id // empty' <<<"$project_json")"; then
    echo "   upload response had no project id" >&2
    summary_sources="$(jq -c --arg source "$name" --arg sha256 "$source_sha256" '. + [{source: $source, source_sha256: $sha256, status: "upload_failed", terminal: false, ready_clips: 0, failed_clips: 0, rejected_candidates: 0}]' <<<"$summary_sources")"
    run_failed=1
    continue
  fi
  echo "   project $id — processing…"

  status="created"
  terminal=false
  failure=""
  view='{}'
  while :; do
    remaining=""
    if ! remaining="$(seconds_remaining "$deadline")"; then
      status="timeout"
      failure="project did not reach a terminal status within ${TIMEOUT_SECONDS}s"
      cancel_project "$id"
      run_failed=1
      break
    fi
    sleep_for="$POLL_SECONDS"
    if ((sleep_for > remaining)); then
      sleep_for="$remaining"
    fi
    sleep "$sleep_for"
    if ! remaining="$(seconds_remaining "$deadline")"; then
      status="timeout"
      failure="project did not reach a terminal status within ${TIMEOUT_SECONDS}s"
      cancel_project "$id"
      run_failed=1
      break
    fi
    if ! view="$(curl_bounded "$remaining" -sf "$HOST/api/projects/$id")"; then
      if seconds_remaining "$deadline" >/dev/null; then
        status="api_error"
        failure="project view request failed"
      else
        status="timeout"
        failure="project did not reach a terminal status within ${TIMEOUT_SECONDS}s"
      fi
      cancel_project "$id"
      run_failed=1
      break
    fi
    if ! status="$(jq -er '.project.status // empty' <<<"$view")"; then
      status="invalid_project_view"
      failure="project view did not contain project.status"
      cancel_project "$id"
      run_failed=1
      break
    fi
    case "$status" in
      complete)
        terminal=true
        break
        ;;
      failed|cancelled)
        terminal=true
        failure="terminal project status: $status"
        run_failed=1
        break
        ;;
      *)
        printf '   %s\r' "$status"
        ;;
    esac
  done

  finished_epoch="$(date +%s)"
  elapsed_seconds=$((finished_epoch - started_epoch))
  out="$RUN_DIR/project-$(printf '%03d' "$source_index")"
  mkdir -p "$out"
  if ! jq . <<<"$view" > "$out/view.json"; then
    jq -n --arg status "$status" --arg failure "$failure" \
      '{project: {status: $status, error: $failure}}' > "$out/view.json"
  fi

  ready_clips="$(jq -r '(.clips // []) | map(select(.status == "ready")) | length' <<<"$view" 2>/dev/null || echo 0)"
  failed_clips="$(jq -r '(.clips // []) | map(select(.status == "failed")) | length' <<<"$view" 2>/dev/null || echo 0)"
  rejected_candidates="$(jq -r '.rejected // 0' <<<"$view" 2>/dev/null || echo 0)"
  selector="$(jq -r '.selector // .project.selector // "n/a"' <<<"$view" 2>/dev/null || echo n/a)"
  if ((failed_clips > 0)); then
    run_failed=1
    if [[ -z "$failure" ]]; then
      failure="$failed_clips clip(s) failed to render"
    fi
  fi
  echo "   → $status: $ready_clips clip(s) ready, $failed_clips failed, $rejected_candidates rejected candidate(s), selector: $selector"
  [[ -z "$failure" ]] || echo "   failure: $failure" >&2

  summary_sources="$(jq -c \
    --arg source "$name" \
    --arg source_sha256 "$source_sha256" \
    --arg project "$id" \
    --arg status "$status" \
    --arg selector "$selector" \
    --arg failure "$failure" \
    --argjson terminal "$terminal" \
    --argjson elapsed "$elapsed_seconds" \
    --argjson ready "$ready_clips" \
    --argjson failed "$failed_clips" \
    --argjson rejected "$rejected_candidates" \
    '. + [{source: $source, source_sha256: $source_sha256, project_id: $project, status: $status, terminal: $terminal, elapsed_seconds: $elapsed, ready_clips: $ready, failed_clips: $failed, rejected_candidates: $rejected, selector: $selector, failure: (if $failure == "" then null else $failure end)}]' \
    <<<"$summary_sources")"
done

totals="$(jq -c '
  {
    sources: length,
    terminal_failures: map(select(.status == "failed" or .status == "cancelled" or .status == "timeout" or .status == "api_error" or .status == "invalid_project_view" or .status == "upload_failed" or .status == "upload_timeout")) | length,
    ready_clips: (map(.ready_clips // 0) | add // 0),
    failed_clips: (map(.failed_clips // 0) | add // 0),
    rejected_candidates: (map(.rejected_candidates // 0) | add // 0)
  }
' <<<"$summary_sources")"
jq -n \
  --arg mode "$MODE" \
  --arg host "$HOST" \
  --argjson sources "$summary_sources" \
  --argjson totals "$totals" \
  '{mode: $mode, host: $host, sources: $sources, totals: $totals}' > "$RUN_DIR/summary.json"

cp "$ROOT/rubric.csv" "$RUN_DIR/rubric.csv"
write_delta "$RUN_DIR/summary.json"
echo ""
echo "Done. Watch every clip, then score $RUN_DIR/rubric.csv (see evals/README.md)."
echo "Summary: $RUN_DIR/summary.json"
echo "Delta:   $RUN_DIR/delta.json"
if ((run_failed)); then
  echo "Eval failed: at least one source did not complete successfully." >&2
  exit 1
fi
