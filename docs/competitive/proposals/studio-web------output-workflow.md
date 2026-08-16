# Studio Web UI & Output Workflow — competitive proposals

**Area:** studio web UI & output workflow · **Date:** 2026-08-12 · **Method:** reverse-skill mode 2 (competitive open-source teardown)

## Scope

Feature area under study: the studio's *web UX around producing and getting clips out of the machine* — import, queueing, project persistence/resume, results organization, export, download. Out of scope (already covered in `docs/competitive/teardown-2026-08.md`): selection/ranking prompts, loudness/energy scoring, framing runs, caption style internals, YAMNet. Nothing here touches crop/pan/framing behavior unless gated behind an explicit new opt-in (see F4).

Competitors read: podcli (`/tmp/competitive-re/podcli`, direct competitor), autoshorts (`/tmp/competitive-re/autoshorts`, Rust/Tauri), yt-short-clipper (`/tmp/competitive-re/yt-short-clipper`), AYSG (fetched from GitHub, not cloned). Our app: `src/api.rs`, `src/state.rs`, `src/store.rs`, `src/domain.rs`, `src/render.rs`, `src/captions.rs`, `src/pipeline.rs`, `static/app.js`, `static/review.js`, `static/index.html`, `static/styles.css`, `docs/PRD.md`.

Ground truth we must not contradict:
- We already persist every project under `~/.clipping-factory/projects/<id>/` (`project.json`, `transcript.json`, `candidates.json`, `render-manifest.json`, `clips/`) — `src/store.rs:9-27`. The UI only remembers the **last** project in `localStorage` (`static/app.js:17`, `cf-project` key) and never surfaces the rest.
- Rendering is two-stage by design: framed **uncaptioned base** (`render_base_clip`) + caption burn (`burn_captions`) — the restyle endpoint proves a re-burn takes seconds (`src/api.rs:435-560`). Any export variant can reuse this.
- Output canvas is hardcoded 1080×1920: `src/render.rs:23-24` (`OUT_W`/`OUT_H`), ASS `PlayResX/Y` in `src/captions.rs:467-468,612-613`, caption x-center math at `src/captions.rs:345`.
- PRD explicitly lists as **non-goals**: batch project queues, multiple source files per project, download-all ZIP files, project library UI (post-MVP), automatic posting (PRD §5, §19.3). Every feature below flags where it conflicts and what sign-off is needed.

---

## F1 — Project history & resume list ("Your projects" on the empty state)

**1. Name:** Recent projects / resume list

**2. User-visible behavior:** The empty state shows a "Recent projects" section: each card = source filename, status badge (`complete` / `in progress` / `failed` / `cancelled`), clip count, relative date, and actions **Open** (resumes the exact processing/results state), **Open folder**, **Delete**. Clicking Open loads `GET /api/projects/{id}` and reconnects SSE — identical to today's refresh-resume path, but for any project, not just the last one. Delete frees disk space (users currently have no way to reclaim the ~16 GB of project data).

**3. Competitor evidence:**
- podcli `src/ui/client/StudioHome.tsx:29-45` — groups clip history by source episode; `:55-62` loads `/history?limit=500` and live-refreshes on SSE `history-updated`; `:73-85` per-card delete (`DELETE /clips/{id}`).
- yt-short-clipper `pages/session_browser_page.py:34-60` — scans the sessions dir for `session_data.json`; `:74-110` renders cards with status badge (`highlights_found`/`completed`/`processing`), highlight count, clip count, date; `:139-216` per-card actions View Session (resume) / View Clips / Open Folder / Delete.
- autoshorts `src-tauri/src/db.rs:131-143` (`list_projects` ORDER BY `updated_at DESC`), `:291-305` (`delete_project`, `rename_project`); README.md "Native Project Manager: Create, open, rename, and delete projects from the dashboard."

**4. Implementation sketch (our seams):**
- `src/store.rs`: add `list_projects() -> Vec<(id, created_at, status, error)>` — read `projects/*/project.json`, skip dirs without `project.json` (upload-in-flight temp dirs are created before the project record — reuse the `exists()` guard at `src/store.rs:82`). Metadata only; never load transcripts/manifests eagerly.
- `src/api.rs`: add to `router()` (line ~30) `GET /api/projects` → `{projects: [...]}`; `DELETE /api/projects/{id}` → 409 when `state.handle(&id).is_running()` (`src/state.rs:99`), else `remove_dir_all(project_dir)` + drop the handle from `handles` map (`AppState.handles`, `src/state.rs:77`).
- `static/app.js`: render the list into `#upload-state` (new `<section id="recent-projects">` in `static/index.html`); fetch on `boot()` and after each `resetToEmpty()`; Open = set `projectId` + `localStorage.setItem("cf-project", …)` + `refetch()` + `connectSse()` (the exact boot path at `app.js:1063-1066`).
- No new dependency, no new runtime service.

**5. Dependencies/risks:** none added. Risk: delete must be destructive-but-honest (confirm dialog naming the source; state that clips in `~/Downloads/Clipping Factory/` are untouched). Do not delete while processing.

**6. Effort:** M (small backend + store, small frontend; delete guard mirrors existing cancel/restyle locking patterns).

**7. PRD fit:** PRD §19.3 ("project history and deletion controls") is explicitly **post-MVP** — this is the natural "product today" pull-forward. No-slop: surfaces only real persisted data. Local-first: filesystem only.

---

## F2 — Multi-file upload queue (sequential, one project per source)

**1. Name:** Drop-in queue for multiple MP4s

**2. User-visible behavior:** The drop zone accepts several MP4s. Each becomes its own project; they process **sequentially** (never concurrent — CPU/disk bounded). A queue strip shows `Next: 2 of 5 · filename`, per-item status, and a "Skip to next" / stop button between items. Each project behaves exactly like today's single-project flow (stages, SSE, results). Refresh-safe: queue state = list of project ids, recovered from the F1 list.

**3. Competitor evidence:**
- podcli `backend/main.py` `handle_batch_clips` (~:320-380): bounded worker pool over a clip list, per-clip progress events (`clip_complete`), per-clip error isolation (`status: error` row without killing the batch), `successful_clips` count in the result.
- AYSG README.md:36 — "Batch processing: `xargs` an entire URL list".

**4. Implementation sketch (our seams):**
- Minimal (recommended first cut): pure-frontend queue in `static/app.js` — `uploadFile(file)` already exists (`app.js:196`); generalize `wireUpload()`'s single-file handlers (`app.js:150-195`) to `File[]`, then loop `POST /api/projects` sequentially, tracking `pendingQueue = [{name, projectId|null, status}]`. Between items, check `/api/setup` `disk_free_gb` and pause with a message when `< 2` (the per-upload disk guard already exists server-side: `api.rs:120-123` `UPLOAD_DISK_RESERVE_BYTES` keeps 1 GiB free per upload).
- Optional backend: `POST /api/projects/batch` (multipart, multiple `file` fields) creating N projects and starting the first via the existing `pipeline::start` (`api.rs:214`). Not required for v1.
- Queue survives refresh only via F1 (list of existing projects) — either both land together, or the queue is a `localStorage` array of `{filename, status}` for the in-flight session.

**5. Dependencies/risks:** none. Risks: disk — each source is a full local copy; the queue must surface "free space" pauses, not fail silently. Long videos × many files = hours of sequential processing; the UI must say so.

**6. Effort:** M (frontend queue state machine; batch endpoint trivial if desired).

**7. PRD fit:** **Conflicts with PRD §5 non-goal "Batch project queues."** Frame honestly: this is a queue of *projects* (one source each), not multi-source projects, and it's opt-in for power users. Needs owner sign-off to pull forward; no-slop contract untouched (nothing here fabricates content).

---

## F3 — Clip rename (edit the output filename)

**1. Name:** Rename clip file

**2. User-visible behavior:** Each ready clip card gets a small "Rename" action. Typing a name updates the `.mp4` filename in the project's `clips/` dir, mirrors it into `~/Downloads/Clipping Factory/<source>/`, updates the manifest, and the Download link now offers the new name. Headline and captions are untouched — this is file organization only.

**3. Competitor evidence:**
- podcli `src/ui/client/ClipDetail.tsx:52-102` — editable clip `title` with dirty state and `patch({ title, caption_style })` persist.
- autoshorts README.md "rename projects" + `src-tauri/src/db.rs:296-305` (`rename_project` SQL update).
- yt-short-clipper `pages/results_page.py:112-127` — per-clip `data.json` carries a user-facing `title` beside the media file.

**4. Implementation sketch (our seams):**
- New `POST /api/projects/{id}/clips/{clip}/rename` in `src/api.rs`: body `{filename}`; validate with the existing `slugify()` (`src/pipeline.rs:610` — alnum + `-` + `_`, no path separators, non-empty, cap length); load manifest (`store.rs:407`), `tokio::fs::rename` in `clips_dir` (guard `final_is_ready`), copy to `output_dir` mirroring `src/pipeline.rs:803`, update `manifest.clips[idx].filename`, `save_manifest`, `handle.emit({"type":"clip",…})`.
- `static/app.js` `clipRow()`: add a rename button in `actions` (prompt-free inline `<input>` preferred, matching the restyle controls pattern at `app.js:514-560`); on success update `view.clips[i].filename` and bump `clipRev[c.id]`.

**5. Dependencies/risks:** none. Risk: filename collisions after rename (`01-x.mp4` vs existing) — reject with a clear message; keep the `NN-` rank prefix optional but default-preserved.

**6. Effort:** S.

**7. PRD fit:** Full fit — output filenames are user metadata (PRD §12 only specifies the default naming). No-slop: never edits speech/captions. Local-first.

---

## F4 — Export variants: 1:1 and 16:9 aspect ratios + resolution (strictly opt-in)

**1. Name:** Export as 1:1 / 16:9 (experimental framing)

**2. User-visible behavior:** On a ready clip, an "Export" menu offers aspect ratios **9:16** (default, current), **1:1**, **16:9**, each at 1080p or 720p. Choosing a non-default ratio re-runs the *framing stage* at the new canvas and burns captions — from the cached base, so it's seconds per variant, not a full pipeline. Every export is labeled "experimental framing" because the face-crop math is tuned for 9:16. The default output never changes.

**3. Competitor evidence:**
- podcli `backend/services/formats.py:13-47` — `FormatSpec` with `vertical` 1080×1920, `horizontal` 1920×1080, `square`; `clip_studio.py:138-141` exposes `--format vertical|horizontal|square`; `_render_bookend` and `_concat` normalize every part to the chosen `width×height` (`clip_studio.py:104-112`, `:156-185`) with the explicit note that a square clip must not be pillarboxed onto a vertical canvas.
- AYSG README.md:35 — "Output format: any aspect ratio, any resolution".

**4. Implementation sketch (our seams):**
- Introduce a `CanvasSpec { width, height }` and thread it instead of the hardcoded constants: `src/render.rs:23-24` (`OUT_W`/`OUT_H`), ASS header `PlayResX/PlayResY` at `src/captions.rs:467-468,612-613`, caption x-center at `src/captions.rs:345`, and the portrait-pad branch in `src/frame.rs:49`. Default stays 1080×1920 so zero behavior change unless the user opts in.
- New `POST /api/projects/{id}/clips/{clip}/export` with `{aspect: "1:1"|"16:9", height: 1080|720}`: if the base exists at current canvas, **re-run `render_base_clip` at the new canvas** (the function already takes `cfg + source + layout + interval`, `api.rs:497`), then `burn_captions` at the new ASS geometry. Clips land as `name--square.mp4` etc.
- UI: per-clip Export control in `static/app.js` `clipRow()`; opt-in only; store choice on the clip record (serde `default`).

**5. Dependencies/risks:** no new deps. Risks: **user-sensitive framing** — face-crop windows, caption safe areas, and blur-pad math assume 9:16; 1:1/16:9 variants need visual QA on single-face, two-face, and no-face fixtures before this is offered beyond "experimental." Disk: each variant is a new file. Effort-heavy because the canvas touches four modules.

**6. Effort:** L.

**7. PRD fit:** PRD §11.1 locks 1080×1920 as the output spec — this proposal *preserves* it as the default and adds opt-in variants. No-slop OK (no content fabrication; variants are re-framings of the same continuous source interval). Aligns with PRD §19.4's "optional user-controlled" direction. Needs owner sign-off to keep the default untouched but expose the option.

---

## F5 — Download all (ZIP of ready clips)

**1. Name:** Download all as ZIP

**2. User-visible behavior:** In the results header, "Download all" streams one `.zip` containing every ready clip (or the single captioned file in caption-only mode). Filenames inside the ZIP preserve the clip filenames.

**3. Competitor evidence:** gap in all three — podcli's library is per-card (download/delete/upload, `StudioHome.tsx:73-85`), yt-short-clipper's results page is per-clip (play / open folder / upload, `results_page.py:139-160`), AYSG outputs file paths for automation (README.md:53 "JSON Output … final clip URLs/paths"). Nobody ships a bulk download; it's a differentiator, not a borrowed pattern.

**4. Implementation sketch (our seams):**
- Option A (zero dependency, first cut): frontend loop in `static/app.js` — for each ready clip, synthesize an `<a download>` click using the same `/api/projects/{id}/clips/{clip}/download` URL as the existing download link (`app.js:495-500`). Works in Chrome/Firefox; Safari blocks more than the first programmatic download — acceptable v1 caveat.
- Option B (reliable): new `GET /api/projects/{id}/download-all` streaming a ZIP from the manifest's ready clips via the `zip` crate (tiny, pure Rust, no binaries; our only new dependency). Stream via `ReaderStream` exactly like `serve_video` (`api.rs:311-338`).

**5. Dependencies/risks:** Option B adds one small pure-Rust crate (flag for sign-off; the project's stance is zero-new-deps-where-possible). Disk: ZIP of several 20-90 s H.264 clips is small (tens of MB); no re-encode needed.

**6. Effort:** S either way.

**7. PRD fit:** **Conflicts with PRD §5 non-goal "Download-all ZIP files."** This was excluded to keep the MVP focused; the owner's ask explicitly includes it, so it needs an explicit sign-off to pull forward. No-slop: ZIP is a pure packaging step.

---

## F6 — Keyboard shortcuts for download & folder (extending the existing swipe review)

**1. Name:** Keyboard-first results: `D` = download current clip, `O` = open output folder

**2. User-visible behavior:** In the swipe-review theater, `D` downloads the clip on screen; in the results grid, `O` opens the output folder. Existing shortcuts (`←`/`→`, `Space`, `1`/`2`/`3`, `R`) already exist and are unchanged.

**3. Competitor evidence:** none of the three competitors are keyboard-first (yt-short-clipper is click-driven customtkinter; podcli and autoshorts are click-driven React/Tauri) — this is a feel differentiator, and we already own the pattern. Our `docs/SWIPE_REVIEW.md` documents the existing theater; `static/review.js:63-99` implements it.

**4. Implementation sketch (our seams):**
- `static/review.js`: in the theater's `keydown` handler (`review.js:86-99`), add `else if (event.key === "d" || event.key === "D")` → trigger a download of `items[index].player.src` by rewriting it to the `/download` URL (same builder as `app.js:496`), e.g. a temporary `<a href=… download=…>`.
- `static/app.js`: `keydown` for `o`/`O` in results state → call the existing `openFolder()` (`app.js:640-652`), honoring its ready-clip guard.

**5. Dependencies/risks:** none. Risk: keep the `typing()` guard (`review.js:82-84`) so shortcuts never fire inside inputs.

**6. Effort:** S.

**7. PRD fit:** Full fit — pure interaction, no content change. No-slop.

---

## F7 — Dark mode

**1. Name:** Dark studio theme

**2. User-visible behavior:** A theme toggle (default: follow the OS) switches the studio between the current light paper theme and a dark variant. Persisted across sessions.

**3. Competitor evidence:** yt-short-clipper is dark-first (`session_browser_page.py:37` `fg_color=("#1a1a1a", "#0a0a0a")`); autoshorts ships a dark UI (README.md screenshot). Dark video studios are the norm; our light theme stands out next to the rendered clips.

**4. Implementation sketch (our seams):** `static/styles.css:5` hardcodes `html { color-scheme: light; }` and `:root` variables at `styles.css:7-17` drive every surface (`--paper`, `--ink`, etc. — no stray hexes per the grep, mostly var-based). Add `@media (prefers-color-scheme: dark)` overrides of the `:root` vars, plus a manual toggle setting `data-theme` on `<html>`; persist in `localStorage` (`cf-theme`, matching the existing `cf-caption-style` pattern at `app.js:30`). Pure CSS + ~10 lines of JS in `boot()`.

**5. Dependencies/risks:** none. Risk: contrast QA on badges/banners (`styles.css:64-74`) and the review theater.

**6. Effort:** S.

**7. PRD fit:** Full fit — presentation only, no content change. No-slop.

---

## TODAY — implementable + testable in one day, ranked by value

| # | Feature | Effort | Why today | Unit-testable |
|---|---|---|---|---|
| 1 | **F1 Project history & resume list** | M | Highest product value: we already persist everything and currently throw it away from the UI; also gives users a way to reclaim disk (delete). | ✅ `Store::list_projects` skips partial dirs; delete refuses while `handle.is_running()` (mirror `state.rs` tests); endpoint returns 409 on running project |
| 2 | **F3 Clip rename** | S | Small, self-contained, immediately useful for organizing output. | ✅ slug validation (reject `..`, `/`, empty, collisions) + manifest rename roundtrip (extend `store.rs` `manifest_roundtrip` test pattern) |
| 3 | **F6 Keyboard shortcuts (D/O)** | S | 15 lines on top of an existing, working keyboard surface. | Manual (no frontend test infra); logic is trivial and guarded by the existing `typing()` check |
| 4 | **F7 Dark mode** | S | Pure CSS + one localStorage key; visual QA only. | Manual/visual |
| 5 | **F5 Download all** | S | Only if owner signs off the PRD §5 non-goal. Frontend loop = zero deps; `zip` crate version gets a streaming unit test. | ✅ (Option B) zip stream contains exactly the ready clips from a fixture manifest |
| 6 | **F2 Upload queue** | M | Best landed right after F1 (queue resume = F1 list); sequential-only keeps CPU/disk safe. | Partial — queue state machine is frontend; backend `POST /api/projects/batch` is testable |
| 7 | **F4 Export variants** | L | Not a one-day task: canvas threading touches `render.rs`, `captions.rs`, `frame.rs` + visual QA across fixtures; keep for a dedicated slice. | — |

Suggested order for the day: **F1 → F3 → F6 → F7** (all clean, no sign-offs), then F5/F2 only with owner sign-off on the two PRD non-goal pulls.

## PRD/decision flags for the owner
- F1: PRD §19.3 post-MVP item — recommend pulling forward (no conflict, only sequencing).
- F2: PRD §5 non-goal "batch project queues" — needs explicit sign-off as opt-in power feature.
- F5: PRD §5 non-goal "download-all ZIP files" — needs explicit sign-off (owner's ask includes it).
- F4: PRD §11.1 default preserved; variant surface needs framing QA sign-off before leaving "experimental".
- Watermark: we never add watermarks — nothing to do; state this when asked.
