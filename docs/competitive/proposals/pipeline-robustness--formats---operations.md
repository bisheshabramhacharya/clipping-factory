# Proposal — Pipeline robustness, formats & operations

Case-init (reverse-skill mode 2): competitor repos podcli (Python, direct
competitor), autoshorts (Rust/Tauri), yt-short-clipper (Python), AYSG
(Anil-matcha/AI-Youtube-Shorts-Generator, Python, fetched from GitHub main on
2026-08-12) vs. our Rust/axum studio at `/Users/bishesha/projects/clipping-factory`.
Scope: pipeline robustness, formats, operations. Read-only teardown; no
modification of competitor code.

Our file map (evidence anchors):
`src/pipeline.rs`, `src/media.rs`, `src/transcribe.rs`, `src/store.rs`,
`src/util.rs`, `src/config.rs`, `src/render.rs`, `src/select/mod.rs`,
`src/select/openai.rs`, `src/select/anthropic.rs`, `src/api.rs`, `src/domain.rs`.
The prior teardown (`docs/competitive/teardown-2026-08.md`) already covers
YAMNet, loudness, encoder fallback, prompt contracts, crop stability, caption
styles, MCP-as-future-surface. This proposal does not repeat those; it fills the
robustness/formats/operations gaps they left open.

Constraint honored throughout: nothing below changes crop/pan/framing behavior
for the existing 9:16 output. Every format/geometry change is a new opt-in
project setting or a per-clip export action. No new runtime services. No heavy
dependencies. No slop: nothing fabricates or rewrites speech.

---

## F1 — Disk-space guard at every heavy stage (pre-flight estimate + mid-run check)

**(1) Name:** Stage-aware disk budget.

**(2) User-visible behavior:** Before the pipeline starts work that writes big
bytes (audio extraction, transcription, layout analysis, renders), the app
checks that the free space is above a per-stage estimate and fails fast with an
actionable message: "This render needs ~2.1 GB; only 1.4 GB free. Free up space
and press Retry." Instead of dying mid-transcription on a full disk (what
happened on this machine this week: a 16 GB project store at 97% capacity
killed a parallel agent run), the user is told the number up front, and after
freeing space Retry resumes from the persisted artifacts.

**(3) Competitor evidence:** podcli never lets a probe failure kill the
pipeline (`encoder.py:74-88` "Absolute fallback — never let encoder detection
break the pipeline"); yt-short-clipper treats mid-run ffmpeg failures as
recoverable by swapping encoder args and retrying (`clipper_core.py:317-345`).
Neither has a disk pre-flight; their episodes are short-form. Ours is a 4-hour
podcast contract (`media.rs:16 MAX_SOURCE_MS`), where disk exhaustion is a real
failure mode and PRD §15 lists "Insufficient disk space" as a first-class edge
case.

**(4) Implementation sketch (our codebase):**
- `src/util.rs` — keep `disk_free_gb` (`util.rs:276-290`); add
  `pub fn estimate_stage_bytes(stage, source: &SourceInfo, clip_count) -> u64`:
  - `extracting_audio`: `duration_ms × 32 KB/s` (16 kHz mono s16 WAV) — exact.
  - `transcribing`: WAV already exists; ~0 new bytes.
  - `analyzing_layout`: frames dir ≈ `clips × 6 frames × (w×h×3)`.
  - `rendering`: `Σ clip_duration × source_bitrate × 1.3` (re-encode factor)
    + same again for the caption burn pass.
- `src/pipeline.rs` — in `Ctx::begin` (`pipeline.rs:157-172`) for the four heavy
  stages, or a helper called at the top of each stage body: compare
  `util::disk_free_gb(data_dir) × 1.15` against the estimate; on shortfall
  return a user-actionable error naming both numbers. Also re-check before each
  individual render in the render loop (`pipeline.rs:548-580`), because base +
  final + copies can exceed the up-front estimate for long clips.
- `src/api.rs` — expose `disk_needed_gb` for the project in `GET /api/projects/{id}`
  so the UI can warn before the user even starts.

**(5) Dependencies/risks:** none; `df -Pk` is already a dependency. Risk: the
estimate can be off for variable-bitrate sources; 15% headroom and a
mid-loop re-check cover it.

**(6) Effort:** S (estimate fn + 3 call sites + tests).

**(7) PRD fit:** §15 (insufficient disk), §7.1 (first-run disk check), §16.3
(operational). Pure robustness, no product change.

---

## F2 — Per-window LLM failure isolation + retry with backoff

**(1) Name:** Windowed selection that survives one bad provider call.

**(2) User-visible behavior:** When the AI key is rate-limited or the model
returns malformed JSON for one transcript window, the whole `selecting_candidates`
stage currently fails and the user must Retry the stage (re-incurring cost and
time). After this change, a failed window is retried once with backoff, and if
it still fails the remaining windows still produce candidates; the results row
shows a warning ("3 of 4 transcript windows selected; window 2 was skipped due
to a provider error"). A fully wedged provider still fails loudly.

**(3) Competitor evidence:**
- AYSG retries invalid model output up to 3 times and tightens the prompt on
  each retry instead of failing the run: `MAX_HIGHLIGHT_API_ATTEMPTS = 3`
  (`highlights.py:68`), retry loop with "Return ONLY valid JSON…" reminder
  (`highlights.py:223-247`).
- podcli treats a backend that "answers with prose where JSON was asked for" as
  a failed attempt and moves to the next backend; the `accept` contract in
  `generate` (`ai_provider.py:246-271`) and `generate_json`
  (`ai_provider.py:345-355`) encode this. `extract_json` strips fences before
  parsing (`ai_provider.py:185-217`).
- Our own `parse_candidates` already strips fences (`select/mod.rs:260-267`);
  the gap is at the loop level: `select/mod.rs:69-92` — `for win in &windows`
  uses `?` on the first error and aborts every remaining window, and there is no
  transient-error retry even though `openai.rs:56-63` already classifies 429/5xx.

**(4) Implementation sketch:**
- `src/select/mod.rs` — restructure `propose()`: per window, run
  `tokio::time::timeout` + up to 2 attempts with exponential backoff
  (e.g. 1s, 4s) on `map_error`-classified 429/≥500 (`openai.rs:56-72`) and on
  parse failures; collect `window_failures: Vec<String>`; continue to next
  window. Return them on `SelectionOutcome` (new field
  `warnings: Vec<String>`).
- `src/pipeline.rs` — after `selecting_candidates` completes, if
  `warnings` is non-empty set `p.warning = Some(...)` (pattern already used for
  low transcription confidence at `pipeline.rs:452-455` and failed renders at
  `pipeline.rs:645-649`).
- `src/domain.rs` — add `warnings` (serde-default) to `SelectionOutcome`/project
  so the UI can render it without schema breakage.
- Unit-testable by extracting the retry policy into a pure fn
  `attempt_with_retry(f: impl Fn() -> Result<Vec<Candidate>>)` and testing
  failure-count → warning behavior with a fake closure (no network).

**(5) Dependencies/risks:** none. Risk: retry costs tokens on genuinely broken
prompts; cap attempts and only retry 429/5xx/parse errors, never user-fixable
401/403 (map_error already separates them).

**(6) Effort:** M (moderate restructuring of `propose()`; small API addition).

**(7) PRD fit:** §15 ("AI key missing, invalid, or rate-limited", "Malformed LLM
JSON"), §8.2 ("The UI must never appear frozen" — a wedged window no longer
freezes the stage), §16.3. No slop: retries select, never fabricate.

---

## F3 — Output format parameterization: 16:9, 1:1 and audio-only export

**(1) Name:** Export formats (opt-in per project).

**(2) User-visible behavior:** A new per-project setting picks the output
format before processing: **Vertical 9:16** (today's default, unchanged),
**Horizontal 16:9** (full source frame, no crop, captions moved to the lower
third), **Square 1:1** (source centered over blurred backdrop on a 1080×1080
canvas — the existing BlurPad geometry, no face cropping), **Audio only**
(M4A/AAC, transcript + headline unchanged). Each finished clip additionally
gets an "Export as…" action that re-renders from the source in a different
format without re-running the pipeline. No existing 9:16 clip or framing
behavior changes unless the user opts in.

**(3) Competitor evidence:**
- podcli `formats.py` is the single source of truth for dimensions, caption
  profile, and duration bounds per format: `FormatSpec` (`formats.py:9-21`),
  `FORMATS = {"vertical", "horizontal", "square"}` (`formats.py:27-57`);
  horizontal sets `reframe=False` (no crop) while vertical/square reframe
  (`formats.py:32-35, 41-44, 50-53`).
- autoshorts already has an audio-only path in its renderer: `has_video` →
  full crop filter, else `-vn` with audio encode (`media.rs:135-166`,
  specifically `media.rs:154-160`).
- podcli `timing_utils.py:4-26` shows per-format caption timing helpers; our
  ASS builder already derives geometry from a style object (`captions.rs`),
  so geometry just needs a FormatSpec input.

**(4) Implementation sketch:**
- `src/domain.rs` — add
  `pub enum OutputFormat { Vertical, Horizontal, Square, AudioOnly }` with
  `serde(rename_all="snake_case")` + `Default = Vertical`; add
  `pub output_format: OutputFormat` to `Project` (default, so old projects load).
- `src/render.rs` — replace `const OUT_W/OUT_H` (`render.rs:18-19`) with a
  `FormatSpec { w, h, reframe: bool, audio_only: bool }`; `build_graph(source,
  layout, format)` picks: Vertical → existing crop/blur; Horizontal → `scale`
  only (never crop); Square → existing BlurPad graph at 1080×1080; AudioOnly →
  skip `[v]` map and encode `-c:a aac` only.
- `src/api.rs` — `GET/POST /api/projects/{id}/format` (validated against the
  enum, opt-in per project before processing); `POST
  /api/projects/{id}/clips/{clip}/export {format}` — re-renders from the
  **source** (not the 1080×1920 base, which is already cropped) with the chosen
  format and writes `~/Downloads/Clipping Factory/<source>/<slug>--<fmt>.mp4`.
- `src/captions.rs` — `CaptionInput` gains `format` so ASS `PlayResX/Y` and the
  caption safe-area origin adapt per format.
- Keep it opt-in: the upload flow stores the choice; the studio shows a format
  picker only on the create-project step and in each clip row's export menu.

**(5) Dependencies/risks:** none (all ffmpeg). Risk: "Export as" re-renders a
full clip (minutes), so show it as a long operation with progress. Square uses
BlurPad only — no face-crop logic is introduced for 1:1, respecting the
framing constraint (BlurPad keeps the full frame visible, it crops nothing).

**(6) Effort:** M–L (domain + render graph + API + caption geometry + UI; the
render-graph change touches the two-pass design but does not break the existing
vertical path).

**(7) PRD fit:** §11.1 fixes 1080×1920 as *the* output; this adds options behind
an explicit user choice — consistent with §6.4 (one deliberate template per
format) and §19 "optional user-controlled caption customization" spirit. No
slop: same source interval, same faithful audio, different geometry.

---

## F4 — Per-clip retry endpoint + render-only resume

**(1) Name:** Retry just the clip that failed.

**(2) User-visible behavior:** Today, "Retry" re-enters the whole pipeline
(cheap only because finished artifacts are skipped) and the user cannot retry a
single failed render from the clip card. After this change, each clip row gets
"Retry this clip"; the failed clip is re-rendered alone while ready clips stay
untouched, and a failed render no longer forces the user through the stage
reset in `pipeline::retry` (`pipeline.rs:76-118`).

**(3) Competitor evidence:**
- PRD §12 is explicit: "Retry only the failed candidate when possible." Our
  render loop already isolates each clip (`pipeline.rs:548-640` — per-clip
  status, base+final ready markers, `store.rs:91-147`), so the isolation
  machinery exists; only the entry point is missing.
- yt-short-clipper splits work into phases with a persisted `session_data.json`
  (`"status": "highlights_found"`, `clipper_core.py:2271-2287, 4833-4848`) and
  resumes from the phase, never from zero. autoshorts keeps a `clips` table per
  candidate with per-clip `status`/`render_log` (`db.rs:52-58`) and updates it
  per render (`db.rs:250-266`).

**(4) Implementation sketch:**
- `src/pipeline.rs` — extract the render loop (`pipeline.rs:548-640`) into
  `pub async fn render_clips(state, id, only: Option<Vec<String>>)` so
  `run()` calls it with `None` and a new entry point calls it with one clip id.
  Reuse `Ctx`, the ass-capability check, and the base/burn two-pass.
- `src/api.rs` — `POST /api/projects/{id}/clips/{clip}/retry`: guard "not
  running" (same as `retry_project`), set that clip `Pending` + clear error in
  the manifest, spawn `render_clips(state, id, Some([clip]))`.
- `src/store.rs` — reuse `final_is_ready`/`mark_final_ready`
  (`store.rs:91-109`) so a retried clip that succeeded meanwhile is skipped.
- Unit test: manifest fixture with one `Failed` clip → `render_clips` with
  `only=[that]` leaves the `Ready` clip untouched in the manifest (mirrors the
  existing `retry_cleanup_...` tests at `store.rs:329-460`).

**(5) Dependencies/risks:** none. Risk: concurrent full-run + clip-retry must
share the operation lease (`state.rs` `handle.operation`); reuse the same
`try_start`/`is_running` guard.

**(6) Effort:** M (one loop refactor + one route + tests).

**(7) PRD fit:** §12 verbatim, §16.1 ("A failed candidate render can be retried
without rerunning successful candidates"), §15 (render process crash). Pure
operations; no product change.

---

## F5 — Capability self-check expansion + cached probe, surfaced in /api/setup

**(1) Name:** Honest startup capabilities.

**(2) User-visible behavior:** The setup screen currently reports ffmpeg/ass/
ffprobe/whisper/model/face-model/disk (`api.rs:123-156`). It does not report
the one thing that determines render speed (hardware encoder), the ffmpeg
version (so "ffmpeg-full vs plain ffmpeg" confusion is diagnosable), or the
free space on the *output* volume (only `data_dir` is checked). After this
change, `/api/setup` reports `videotoolbox`, `ffmpeg_version`, both volumes'
free space, fonts dir state, and the model size; the probe result is cached on
disk keyed by the ffmpeg binary fingerprint so repeated `/api/setup` calls are
instant (the first call runs the probe).

**(3) Competitor evidence:**
- podcli probes hardware encoders once and caches to `encoder.json` keyed by
  `ffmpeg path:mtime:size` fingerprint — "Probing runs ffmpeg twice (~1.6s on
  macOS) — huge startup win" (`encoder.py:118-152`); it also *tests* the encoder
  with a real tiny encode rather than trusting the encoder list
  (`encoder.py:89-110`).
- Our `render.rs:179-195` already probes `h264_videotoolbox` via a
  process-local `OnceLock`, but nothing surfaces it; the probe runs once per
  process *twice* (base + burn) and never persists.
- yt-short-clipper pre-flights GPU encoder args at startup and disables them at
  runtime on signature-matched failure (`clipper_core.py:216-226, 305-345`).

**(4) Implementation sketch:**
- `src/util.rs` — add `ffmpeg_caps(bin) -> Result<Caps>` (version string,
  `has_ass`, `has_h264_videotoolbox`) with an on-disk JSON cache under
  `<data_dir>/cache/ffmpeg-caps.json` keyed by `(path, mtime, size)` following
  podcli's fingerprint; invalidate on change. Keep a process `OnceLock` on top.
- `src/render.rs` — `video_encode_args` (`render.rs:179-210`) reads the same
  cache instead of probing privately.
- `src/api.rs` — `setup_status` (`api.rs:123-156`) adds `ffmpeg_version`,
  `videotoolbox`, `hw_encode` (=videotoolbox), `fonts_dir_ok`,
  `whisper_model_mb` (already there as `model_mb`), `disk_free_gb_output` for
  `output_root`, and `max_source_ms`.
- Tests: cache round-trip and fingerprint invalidation (temp dirs, like
  `util.rs` tests).

**(5) Dependencies/risks:** none. Risk: caching a probe from a broken ffmpeg —
fingerprint on mtime+size covers upgrades; treat empty caps as "unprobed" and
re-probe.

**(6) Effort:** S–M.

**(7) PRD fit:** §7.1 first-run check verbatim, §15 (FFmpeg/FFprobe missing,
ASS missing — the current hard-fail at `pipeline.rs:516-536` can now be
pre-announced on the setup screen). Operational only.

---

## F6 — Project delete + aggressive temp pruning (operations)

**(1) Name:** Reclaim space, delete a project.

**(2) User-visible behavior:** The studio gains a per-project "Delete" action
that removes the project directory (source copy, frames, bases, clips,
transcripts) after a confirm dialog; a "Clear temp data" action prunes
`frames/`, `.part-*`, `.ass`, stale `audio.wav` without touching completed
clips. This machine's `~/.clipping-factory/projects` currently holds 16 GB of
test projects with no way to reclaim them from the UI — PRD §13 explicitly
permits a project-level delete "if it is needed to clear generated data during
development."

**(3) Competitor evidence:**
- autoshorts: `delete_project` cascades transcripts/candidates/clips/copy via
  FK (`db.rs:406-410`; schema `db.rs:40-93`).
- yt-short-clipper: `cleanup()` removes the per-session `_temp` dir
  (`clipper_core.py:3582-3587`); per-phase session dirs keep resume artifacts
  tidy.
- Our `cleanup_partial_files` (`store.rs:149-256`) already distinguishes
  proven-complete from partial; a delete API just reuses the same path walking.

**(4) Implementation sketch:**
- `src/store.rs` — `pub async fn delete_project(&self, id) -> Result<()>`:
  best-effort `remove_dir_all(project_dir)`, tolerate missing dir; never touch
  the output folder (user's files stay).
- `src/api.rs` — `DELETE /api/projects/{id}` (guard: refuse if a run is active,
  same `handle.operation` lock as `retry_project` at `api.rs:566-576`).
- `src/api.rs` or a timer — `POST /api/projects/{id}/cleanup` calling
  `cleanup_partial_files` (already safe and tested) so users can reclaim the
  frames/temp without deleting anything complete.
- Tests: store-level delete + "delete does not touch output_dir" (fixture dirs,
  matching the existing cleanup tests).

**(5) Dependencies/risks:** none. Risk: accidental deletion — the confirm
dialog is mandatory and the endpoint is idempotent (404 on missing project).

**(6) Effort:** S.

**(7) PRD fit:** §13 last bullet (project delete when needed to clear generated
data), §15 (insufficient disk). Operations only; no content behavior change.

---

## F7 — MCP server + JSON automation surface (spec)

**(1) Name:** Agent-controllable studio (spec + zero-dep JSON first).

**(2) User-visible behavior:** Future: an agent (Claude/Codex/Cursor) can drive
the studio via MCP — create a project from a local path, start processing,
poll status, retry, download clips. Near-term, this proposal ships the
zero-dependency half: `POST /api/projects/import` (create from a local file
path instead of multipart upload) and `GET /api/projects/{id}/export.json`
(a full machine-readable bundle of project, transcript, selection, manifest,
and clip statuses). The MCP adapter itself is specced but gated behind a
feature flag because it adds a crate.

**(3) Competitor evidence:**
- podcli has a full integration registry for agent tool surfaces:
  `ToolSpec {name, description, handler, input_schema, tags}` +
  `IntegrationBase`/`IntegrationRegistry` (`integrations/base.py:16-60`,
  `integrations/manager.py:26-49`), and an MCP server so agents drive the CLI
  (flagged in the prior teardown).
- yt-short-clipper ships `--output-json` for automation.
- Our API already exposes `GET /api/projects/{id}`, `GET .../events` (SSE),
  `POST .../process|cancel|retry`, and per-clip download (`api.rs:33-58`) — the
  JSON surface is 90% there; only import-from-path and a bundled export are
  missing.

**(4) Implementation sketch:**
- `src/api.rs` — `POST /api/projects/import {path}`: validate the path is an
  existing file, copy it into the project dir (reusing the upload streaming
  path at `api.rs:277-360` minus multipart), return the project id. Guard: only
  accept paths the user explicitly supplies; log no contents.
- `src/api.rs` — `GET /api/projects/{id}/export.json`: assemble
  `{project, transcript, selection, manifest}` from the store (all files
  already exist on disk as JSON — this is serialization, not a new store).
- `mcp/` (feature-flagged, post-MVP): spec — stdio MCP server exposing tools
  `create_project_from_path`, `get_project`, `start_processing`, `cancel`,
  `retry`, `retry_clip`, `list_clips`, `download_clip`, each mapping 1:1 onto
  the handlers above. Dependency: an MCP Rust SDK (e.g. `rmcp`/`mcp-rs`); the
  spec keeps the handler layer SDK-free so the adapter is a thin shell.

**(5) Dependencies/risks:** the JSON endpoints add nothing; the MCP server adds
one crate and a process — gate it behind `--enable-mcp` / env var and keep it
out of the default `npm run app` path. Security: `import` must not follow
arbitrary symlinks out of the projects dir (copy, don't hardlink; resolve
symlinks before validation).

**(6) Effort:** S (JSON endpoints + tests) / L (MCP adapter, only if pursued).

**(7) PRD fit:** post-MVP automation surface (consistent with §19's sequencing
— this must not ship before quality gates pass); no-slop: export is verbatim
project state, never invented metadata.

---

## TODAY — single-day implementable features, ranked by value

All six are implementable *and unit-testable in one day* (pure functions, store
fixtures, API JSON; no new services, no network-dependent tests). Ranked:

1. **F4 — Per-clip retry endpoint.** Highest PRD value (§12 verbatim), smallest
   surprise: the isolation machinery already exists in the render loop; the work
   is one refactor + one route + manifest-fixture tests. The other features get
   safer because retry stops being a whole-stage reset.
2. **F1 — Disk-space guard.** The failure mode that actually bit this machine
   this week (97% full volume killing long jobs). Estimate fn is pure math;
   tests are trivial; call sites are 3–4 one-liners.
3. **F6 — Project delete + temp pruning.** Reclaims the 16 GB test-project
   store today; reuses the tested `cleanup_partial_files`; store-level delete
   tests mirror existing cleanup tests.
4. **F5 — Capability cache + /api/setup expansion.** Small, self-contained;
   cache keyed by ffmpeg fingerprint has clean unit tests; unblocks the UI from
   explaining "install ffmpeg-full" before the user uploads.
5. **F2 — Per-window failure isolation + retry.** Doable in a day if the retry
   policy is extracted as a pure function (fake provider closure in tests);
   slightly riskier than the above because it reshapes `propose()`.
6. **F7 (JSON half) — import-from-path + export.json.** Two routes, both thin;
   the MCP adapter is explicitly NOT today (new crate, needs its own design
   pass).

Not today: F3 (output formats) touches domain + render graph + captions + API +
UI and deserves its own slice; MCP adapter (F7) needs the dependency decision.

---

*Method notes: podcli/autoshorts/yt-short-clipper read from local clones at
`/tmp/competitive-re/`; AYSG files fetched from
`raw.githubusercontent.com/Anil-matcha/AI-Youtube-Shorts-Generator/main/`
(highlights.py) — timestamped 2026-08-12. Evidence is file:line; no competitor
code was copied (licenses: podcli AGPL, AYSG MIT, yt-short-clipper MIT,
autoshorts Apache-2.0 — techniques only).*
