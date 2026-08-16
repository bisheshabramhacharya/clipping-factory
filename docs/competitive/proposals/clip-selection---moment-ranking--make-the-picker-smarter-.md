# Proposal: clip selection & moment ranking — make the picker smarter

**Area:** editorial selection engine (PRD §9) · **Method:** reverse-skill MODE 2 (competitive teardown)
**Date:** 2026-08-12 · **Status:** proposal (no `src/` changes)

## Scope (case-init)

Competitor set: `podcli` (direct competitor), `autoshorts` (Rust/Tauri), `yt-short-clipper`,
`AI-Youtube-Shorts-Generator` (AYSG). Feature area: **moment selection & ranking** — how each
project finds, scores, dedupes, and boundary-fits clips, plus reaction-aware signals
(YAMNet). Read-only; no builds, no clones, no model downloads.

## Methodology

Read the selector/scoring modules only (per MODE 2), cited as `file:line` below:

| Repo | Files read |
|---|---|
| podcli | `backend/services/audio_events.py`, `audio_analyzer.py`, `claude_suggest.py`, `clip_generator.py`, `transcript_packer.py`, `plans/moment-detection.md` |
| autoshorts | `src-tauri/src/llm.rs` |
| yt-short-clipper | `clipper_core.py` (`get_default_prompt`, `find_highlights`) |
| AYSG | `shorts_generator/highlights.py` (fetched from GitHub, not cloned) |
| ours | `src/select/mod.rs`, `src/select/heuristic.rs`, `src/select/openai.rs`, `src/select/anthropic.rs`, `src/energy.rs`, `src/validate.rs`, `src/domain.rs`, `src/pipeline.rs` |

Existing teardown `docs/competitive/teardown-2026-08.md` already covers the loudness
z-score (implemented), the YAMNet *idea* (deferred), encoder fallback, and framing — this
proposal does not repeat those; it details the deferred YAMNet item and the ranking/boundary/
multi-pass gaps around it.

---

# Features

## Feature 1 — Reaction-anchored moments (YAMNet) with backwards extension

1. **Name:** Reaction-anchored moments (optional, self-disabling).
2. **What it does (user-visible):** the app detects laughter/cheering/screaming in the
   audio and (a) boosts candidates that contain or lead into a reaction, (b) extends a
   clip's start **backwards** so the joke/story that *caused* the reaction is included and
   the laugh lands as the payoff. If the model or runtime is missing, the feature silently
   turns off and the picker behaves exactly as today.
3. **Competitor evidence:**
   - podcli `audio_events.py:43-49` — laughter family collapsed by substring
     (`laugh/giggle/chuckle/snicker/chortle`), reaction channels = laughter/cheering/screaming,
     `REACTION_THRESHOLD = 0.15`.
   - `audio_events.py:89-95` — `is_available()`: runtime+model missing ⇒ empty results, callers degrade.
   - `audio_events.py:156-174` — windowed inference (300 s chunks), `[frames,521]` scores, ~0.48 s hop.
   - `audio_events.py:221-246` — absolute-calibrated scores: `min(10, peak*12)`, explicitly
     *not* z-scored ("only meaningful relative to a video's own baseline" for RMS, unlike
     YAMNet probabilities).
   - `claude_suggest.py:64-75` — anchor prompt block: "The moment that CAUSED each reaction
     sits just before its anchor. Clips that contain or lead into an anchor are strong
     candidates."
   - `plans/moment-detection.md:24` — FunnyNet-W: "a funny moment is an n-second clip
     *followed by* laughter, audio carries >50% of the decision, optimal look-back ≈ 8s".
   - `plans/moment-detection.md:50-56,71-76` — model = self-contained YAMNet ONNX
     (16 MB, Apache-2.0), raw 16 kHz waveform in, log-mel baked into the graph (no librosa);
     runtime = `onnxruntime` CPU, the only new hard dep. OpenCV-DNN **cannot** run it
     (`:53` "dynamic 'zero' shapes are not supported") — so the zero-new-dep route is out.
   - `plans/moment-detection.md:79-84` — verified: torch/tensorflow absent at runtime.
   - Model availability verified live: `huggingface.co/zeropointnine/yamnet-onnx`, Apache-2.0,
     not gated, `yamnet.onnx` = 16,093,603 bytes ≈ 15.3 MiB.
4. **Implementation sketch (ours):**
   - New module `src/reactions.rs`: `ReactionProfile { per_second: Vec<(u8,u8,u8)> }` or a
     sparse `Vec<ReactionAnchor{ t_ms, kind }>` persisted as `reactions.json` via the
     `store.rs` pattern used by `energy.json` (`store.rs:52-59`).
   - **Seam:** run it in the same pipeline window as `energy::measure` — `pipeline.rs:444`
     (after transcribe, before the WAV is deleted at `pipeline.rs:448`), advisory like
     energy ("never fail the stage on it"). The 16 kHz mono WAV that whisper.cpp produces is
     exactly YAMNet's input, so no new extraction.
   - **Runtime:** `ort` crate (actively maintained, crates.io, updated 2026-07-28) with
     `download-binaries` (CPU) feature. Alternative: `tract-onnx` (pure Rust, no C dep,
     slower). Do NOT call out to Python (repo is Rust-native; whisper.cpp replaced the PRD's
     original Python worker).
   - **Wiring:** extend `select::propose` signature (`select/mod.rs:37`) with
     `reactions: Option<&ReactionProfile>` mirroring `energy`; feed anchors into
     `window_prompt` (`select/mod.rs:224`) as an "AUDIENCE REACTION ANCHORS" block
     (port of `claude_suggest.py:64-75`), and add a 0..+3 composite boost in
     `heuristic.rs` composite (mirror podcli's `REACTION_BLEND_WEIGHT=0.2` at
     `claude_suggest.py:782-783`).
   - **Backwards extension:** deterministic post-pass in `validate.rs` (after
     `snap_to_words`, `validate.rs:210`): if a reaction anchor sits within [start, start+3 s],
     move `start_ms` back up to 8 s to the previous sentence start; caps: never below
     MIN_MS, never push `end - start` past MAX_MS, always land on a sentence boundary.
   - **Model download:** 16 MB from HF at first run, reusing the existing model story in
     `transcribe.rs:27-30` (whisper ggml download with size in the error message; PRD §7.1
     already specifies a download-progress UX). Self-disables when missing
     (`is_available()` pattern).
5. **Dependencies / risks:**
   - `ort` pulls onnxruntime binaries (~30–50 MB build output, noticeably longer compile).
     Build-time risk on macOS arm64 is low (officially supported) but must be verified in a
     branch before committing to it. `tract-onnx` avoids the native dep at a large
     inference-speed cost (YAMNet is MobileNet-class; tract ≈ roughly realtime-or-slower on
     an M-series for 16 kHz audio, ort ≈ 3–10× faster).
   - 16 MB first-run download (matches existing whisper-download UX; not a runtime service).
   - CPU cost: order of minutes for a 1–2 h podcast (windowed); measure on the eval set
     before shipping as always-on — gate behind "when a key is configured" or a checkbox if
     it slows the pipeline.
   - Licenses: model Apache-2.0, onnxruntime MIT — clean.
   - **Constraint check:** touches selection only, never framing/crop — no user-sensitive
     visual behavior.
6. **Effort:** L (new dep, model plumbing, inference module, anchor plumbing, extension
   pass, tests). The non-inference half (ReactionProfile type, anchor→prompt/boost wiring,
   backwards-extension pass, self-disable) is independently testable with a synthetic
   profile, so it can be built before the ort spike lands.
7. **PRD fit:** §9.1 (windowing + ranking), §16.2 (improve selection before adding
   effects), §6.1 (only start/end chosen — faithful), local-first (§13: model local,
   transcript-only to the LLM). No fabricated content — reactions are real audio events.

---

## Feature 2 — Pseudo-reaction anchors from transcript + energy (model-free 80%)

1. **Name:** Pseudo-reaction anchors (cheap laughter/emphasis detection, zero deps).
2. **What it does (user-visible):** without any new model, the picker gets "a reaction
   happened near here" anchors from data it already has: a loud burst immediately after a
   sentence boundary, laughter-ish tokens in the transcript ("ha", "(laughs)", "that's
   hilarious"), and our existing Q&A exchange pattern. Anchors feed the same backwards-
   extension and boost logic as Feature 1, so the ~80% of reaction value is captured today
   and Feature 1 becomes a drop-in upgrade later.
3. **Competitor evidence:**
   - podcli `plans/moment-detection.md:24` — the "followed by laughter" shape + ~8 s look-back
     is the core transferable idea, independent of the model.
   - podcli `audio_events.py:31-34` — a laugh anti-correlates with speech at the frame
     level, which is why it works as an *anchor*: the clippable moment is adjacent, not
     identical, to the reaction.
   - podcli `audio_analyzer.py:109-140` — energy z-scores (`z_avg*0.4 + z_peak*0.6`) feed
     segment scoring; we already do the z-score half in `energy.rs:179-210`.
   - ours `heuristic.rs:435-441` — `has_reaction_exchange` already detects
     "question → short reply ≤1.5 s" turns; this is the seed of the anchor concept.
4. **Implementation sketch (ours):**
   - `src/energy.rs` — new pure function
     `reaction_anchors(profile: &EnergyProfile, t: &Transcript) -> Vec<ReactionAnchor>`
     (Anchor = { t_ms, kind: LoudBurst|LaughterCue|QaTurn }). Three detectors:
     (a) z-scored loudness: a second whose z ≥ ~2.0 inside a 1.5 s window that starts within
     ~1 s *after* a sentence boundary (the reaction follows the setup); (b) transcript
     tokens: normalized window contains `ha`, `haha`, `laugh`, `hilarious`, `(laughs)`,
     `that's funny`; (c) reuse `has_reaction_exchange`.
   - `src/select/heuristic.rs:131` — in the per-window scoring loop, when a candidate's
     start sits ≤3 s after an anchor, add 0..+2 to composite and (optionally) extend start
     backward (same caps as Feature 1).
   - `src/select/mod.rs:224` — append an anchors block to `window_prompt` for the LLM path
     (port of `claude_suggest.py:64-75`).
   - Testable entirely with synthetic profiles + fixture transcripts (pattern already in
     `energy.rs` tests and `heuristic.rs` fixtures).
5. **Dependencies / risks:** none new. Risk: false positives (loud music ≠ reaction; laugh
   tokens are whisper-dependent). Mitigation: anchors are advisory boost + extension only,
   never a hard gate; validator thresholds unchanged. When Feature 1 lands, anchor kinds
   from YAMNet replace the heuristics per-kind — same struct, same plumbing.
6. **Effort:** S.
7. **PRD fit:** high — no-slop (chooses real intervals), local-first, and it directly
   serves §16.2's directive to improve selection rather than visuals when quality fails.

---

## Feature 3 — Deterministic boundary tightening (trim weak openings, snap ends to sentence boundaries)

1. **Name:** Boundary tightening.
2. **What it does (user-visible):** clips stop starting with "So, um, well…" dead air and
   stop ending mid-thought. The validator deterministically trims a weak opening run
   (≤3 s, never into a hook) and extends the end forward to the next sentence-ending word
   (≤3 s, never past the 90 s cap). The words themselves are untouched — only the chosen
   start/end move, exactly the kind of fix the PRD's "human editor" bar wants.
3. **Competitor evidence:**
   - podcli `clip_generator.py:47-53,55-133` — `_WEAK_OPENING_WORDS`
     (`so/well/okay/like/you/know/right/yeah/actually/basically` + fillers), `_trim_weak_opening`
     with `max_trim=3.0 s`, `min_gain=0.25 s`, "never trim … into a likely hook ('?'/'!')".
   - podcli `clip_generator.py:331-363` — `_snap_to_sentence_end`: forward-only, ≤3 s,
     stops at a speaker change, requires `.!?`.
   - yt-short-clipper `get_default_prompt` (`clipper_core.py:404-586`): "Start at the exact
     moment the hook hits — no preamble, no 'so', no 'well'"; "NEVER cut mid-sentence".
   - ours: `validate.rs:210-228` snaps only to *word* boundaries, not sentence boundaries;
     `heuristic.rs` already starts on sentence starts (LLM path is the sloppy one).
4. **Implementation sketch (ours):**
   - `src/validate.rs` — inside `validate`, after `snap_to_words` (`validate.rs:210`):
     1. **Trim start:** while the first words of the interval are in a `WEAK_OPENING`
        list (port of podcli's set), advance `start_ms` to the first non-weak word; caps:
        ≤3 s of trim, stop if any trimmed word contains `?`/`!`, require ≥0.25 s gained,
        never below the 20 s MIN_MS window (constant at `validate.rs:11`).
     2. **Snap end:** if the last word does not end a sentence, advance `end_ms` to the
        next word ending with `.!?` (≤3 s forward); never exceed MAX_MS.
   - Re-run quote-zone checks after moving boundaries (opening/closing quote zones are
     computed from the excerpt in `validate.rs:88-107` — already order-independent).
   - Unit tests: reuse the `transcript()` fixture helpers in `validate.rs` tests; new
     fixtures for "opens with So/well", "ends mid-thought", "don't trim a hook question",
     "respect 90 s cap".
5. **Dependencies / risks:** none. The only risk is trimming a deliberate conversational
   opener; podcli's min_gain + never-into-hook guards, plus keeping the change purely in
   the deterministic validator (final authority per PRD §9.3), makes it reviewable.
6. **Effort:** S.
7. **PRD fit:** direct — §6.3 "Start on a sentence or natural conversational boundary…
   End after the idea… resolves", §9.1 "Snap accepted start and end values to real word
   timestamps". Faithfulness preserved (no words removed from the audio stream).

---

## Feature 4 — Global re-ranking pass over the merged shortlist (the PRD's missing final pass)

1. **Name:** Shortlist re-ranking pass.
2. **What it does (user-visible):** on long podcasts, the app already asks several windows
   and merges the results — but today it keeps whatever each window returned, in merge
   order. With this feature it runs one final pass: all merged candidates (compact: start,
   end, headline, first sentence) are sent back to the LLM once to be ranked and pruned to
   the planning target, so rank order reflects a *global* view instead of per-window
   opinion. If the re-rank call fails, the merged order is kept (graceful degradation).
3. **Competitor evidence:**
   - PRD §9.1 (ours, the contract): "For long transcripts, process overlapping transcript
     windows, merge candidates, then run **one final ranking pass over the shortlist**."
   - ours `select/mod.rs:37-86` — the provider path merges with `dedupe_similar`
     (`mod.rs:333`) and **never re-ranks**; the final pass the PRD promises is missing.
   - podcli `claude_suggest.py:321-330` — `_select_top_by_score`: "Ranking by score must
     come before truncation — otherwise the earliest clips ship, not the best ones."
   - podcli `claude_suggest.py:610-640` — several independent searches over the same
     transcript, then "Keeping the union and re-ranking beats picking one set".
   - AYSG `highlights.py:251-270` — merged chunk outputs sorted by score before overlap
     suppression (score-first, then dedupe).
4. **Implementation sketch (ours):**
   - `src/select/mod.rs` — after the windows loop + `dedupe_similar`, if
     `windows.len() > 1 && all.len() > target`: build a `rerank_prompt` listing each
     candidate (index, `MM:SS` range, headline, first ~12 words), asking the LLM to return
     an ordered JSON array of the best `target` indices with a one-line reason each.
   - Reuse `parse_candidates`-style tolerant parsing (`mod.rs:284`); on parse/HTTP failure,
     log and return the merged order unchanged (never fail the stage on the re-rank).
   - `openai.rs`/`anthropic.rs` need no changes (they are generic complete() calls).
   - Tests: prompt builder shape + "re-rank failure ⇒ merged order preserved" with a stub.
5. **Dependencies / risks:** one extra LLM call per long source (cost ~1–2 k tokens in,
   ~500 out). No new deps. Risk: the model overruling a locally-stronger candidate — but
   the validator still gates everything afterward (`validate.rs:19`), so the re-rank only
   affects *order and pruning among passing candidates*.
6. **Effort:** M (prompt + parse + merge plumbing; HTTP path needs a key for end-to-end).
7. **PRD fit:** this *is* PRD §9.1 verbatim; zero-slop (ordering only, no content change).

---

## Feature 5 — Under-covered region top-up pass (diversity on the LLM path)

1. **Name:** Coverage-aware top-up.
2. **What it does (user-visible):** for a 2-hour podcast, today one dense 30-minute stretch
   can monopolize the candidate slots. The picker now checks where accepted candidates
   actually landed and, when whole regions of the source have zero survivors, runs one
   targeted pass over the most under-covered region — still through the same validator, so
   nothing weak sneaks in. Result: clips spread across the episode instead of clustering.
3. **Competitor evidence:**
   - podcli `claude_suggest.py:853-960` — `suggest_more_with_claude`: buckets the timeline,
     sorts buckets by coverage ratio ascending, searches the least-covered first, stops
     early once `top_n` is reached.
   - podcli `claude_suggest.py:253-271` — `_should_bucket_initial_selection`:
     ≥45 min / ≥180 segments / ≥18 k chars ⇒ bucketed search with per-bucket top_n.
   - podcli `claude_suggest.py:273-290` — `_dedupe_clips_by_range` >50% overlap of the
     shorter clip, keep higher-scored (we do the same at `mod.rs:333-355`, 55%).
   - ours `heuristic.rs:364-376` — the *local* selector already enforces positional
     diversity (max ~half the slots per third of the source); the LLM path has no
     equivalent, which is exactly this gap.
4. **Implementation sketch (ours):**
   - `src/select/mod.rs` — new helper `coverage_gaps(candidates, source_duration_ms,
     buckets: usize) -> Vec<(usize_start_ms, usize_end_ms)>` (pure, testable): split the
     source into ~6 buckets (podcli's default), mark buckets with ≥1 surviving candidate
     as covered.
   - In `propose` (`mod.rs:37`): if after merge+dedupe any bucket is empty and the global
     count is below `target * 1.5`, build one extra `Window` for the emptiest region and
     run it through the same provider call path with `per_window` floor 2; append results.
   - Validator unchanged — the top-up merely *proposes more*; rejection rules still decide.
   - Tests: coverage calc on synthetic candidate sets (region-with-none detected, hot
     region excluded).
5. **Dependencies / risks:** up to one extra LLM call. Risk of padding with weak
   candidates is nil because the validator is the gate; PRD §6.2's "weak moments must be
   rejected" is preserved (we never lower thresholds to fill a region).
6. **Effort:** S/M.
7. **PRD fit:** §9.1 windowing + §6.2 (distinct moments, quality-over-quota); no visual
   surface touched.

---

## Feature 6 — Strict output schema + honest-score hardening

1. **Name:** Schema-strict proposals.
2. **What it does (user-visible):** fewer "this clip got accepted but the scores say
   otherwise" surprises. The provider is asked for our exact 8-field score object as
   integers 1–5 under a JSON-schema contract (OpenAI structured outputs), the system prompt
   gains a self-check step, and any candidate that omits the whole scores object is rejected
   as malformed instead of silently defaulting to 1s.
3. **Competitor evidence:**
   - yt-short-clipper `clipper_core.py:404-586` — the most aggressive prompt contract in
     the set: exactly N clips, exactly 6 fields ("PERSIS 6 FIELD – TIDAK BOLEH
     LEBIH/KURANG"), a mandatory pre-return SELF-VALIDATION checklist, and an anchored
     `virality_score` rubric (8–10 = controversial/emotional/confession/statement/punchline;
     5–7 = insight/story/light humor; 1–4 = neutral info) at `clipper_core.py:511-550`.
   - autoshorts `llm.rs:34-60` — single prompt for five providers, plus
     `parse_candidate_json` (`llm.rs:390-470`) that flexibly locates the array and
     normalizes scores (0–10 / 0–100 → 0–1) — i.e., defensive parsing of sloppy output.
   - AYSG `highlights.py:127-162` — `_sanitize_highlights` coerces ints/floats, clamps
     score to 0–100, *skips* invalid entries rather than inventing values.
   - ours `openai.rs:16-47` — only `response_format: {"type":"json_object"}` (openai.rs:21);
     `parse_candidates` (`mod.rs:284-332`) clamps every score with defaults of 1 — a
     candidate that omits `scores` entirely sails through as all-1s and only fails the
     validator by luck of thresholds.
4. **Implementation sketch (ours):**
   - `src/select/openai.rs` — upgrade `response_format` to
     `{"type":"json_schema","json_schema":{...}}` (OpenAI structured outputs) pinning:
     `candidates[]`, all 9 fields required, `scores.*` as integer 1–5. Keep `json_object`
     for Anthropic (forced tool-use is fiddly) — the defensive parser covers both.
   - `src/select/mod.rs:284` — in `parse_candidates`: if `c.scores.is_none()`, push a
     rejected-candidate record with reason "scores object missing" instead of defaulting;
     keep clamping for out-of-range *values* but reject non-integer text that fails to
     parse (currently `clamp_score` rounds 2.6 → 3, hiding laziness).
   - `SYSTEM_PROMPT` (`mod.rs:186`) — append the self-check line: "Before returning, verify
     every score is an integer 1–5 and start_ms < end_ms; weak moments must score low."
   - **"Exactly N" evaluation (special focus):** do **not** copy yt-short-clipper's
     "HASILKAN TEPAT N" (`clipper_core.py:415`) — it forces padding and empty-array bans,
     which directly violates PRD §6.2 (quality over quota) and the no-slop contract. Its
     *mechanism* (schema + self-check + anchored score rubric) is adopted; the quota is not.
     AYSG's "ask ~2×, then prune" (`highlights.py:210`) is the compatible version and is
     already what our `proposal_count = ceil(target*1.5)` does (`mod.rs:19-24`).
   - Tests: malformed-scores rejection, integer-only enforcement in the parser, self-check
     string presence.
5. **Dependencies / risks:** none new. `json_schema` requires a recent OpenAI model
   (gpt-4o/4.1-mini class); fall back to `json_object` when the model rejects the format
   (map the 400 to the existing `map_error` path, `openai.rs:60-77`).
6. **Effort:** M (small code; the API-format branch and model-version matrix need a key to
   verify end-to-end).
7. **PRD fit:** §9.2 "Required structured output" — this enforces the contract the PRD
   already wrote; validator remains final authority (§9.3).

---

## Feature 7 — Content-type / density pre-pass that conditions the prompt

1. **Name:** Genre-aware prompting.
2. **What it does (user-visible):** the picker spends one tiny call (~3 k chars, first
   minutes of the source) classifying the episode as podcast/interview/tutorial/debate and
   its density (low/medium/high). The selection prompt is then conditioned: a dense
   interview gets "prefer specific claims and numbers", a banter-heavy show gets "prefer
   stories and exchanges". Same rubric, same thresholds — the *priority emphasis* adapts.
   Nothing user-visible except better picks.
3. **Competitor evidence:**
   - AYSG `highlights.py:163-171` — `detect_content_type` samples the first 25 segments /
     3000 chars, one small call, degrades to `{"other","medium"}` on failure.
   - AYSG `highlights.py:36-62` — the classification is interpolated into the system
     prompt ("Content type: {content_type} | Density: {density}").
   - podcli `claude_suggest.py:150-166` — inlines a knowledge base describing the *show's*
     voice and content types into the prompt (heavier, channel-specific variant).
4. **Implementation sketch (ours):**
   - `src/select/mod.rs` — before the window loop in `propose` (`mod.rs:37`): one
     `openai::complete`/`anthropic::complete` call with a 15-line prompt over
     `transcript.sentences[..25]` asking for `{"content_type":"...","density":"..."}` only;
     parse tolerantly; on any failure use `("other","medium")` (AYSG pattern) — never fail
     the stage.
   - Interpolate into `window_prompt` (`mod.rs:224`) as one advisory line: "Episode genre:
     {ct} ({density}). Emphasize {…}" — a prompt-only nudge; validator thresholds and
     no-slop rules are untouched.
   - `window_prompt` signature gains the two strings; tests assert the line renders and is
     omitted on degrade.
5. **Dependencies / risks:** one extra LLM call (~1–2 k tokens). Risk: the classification
   nudges the model into padding "on-genre" filler — mitigated because every candidate
   still passes the same 8-score rubric + validator, and the nudge is one line, not a rule
   rewrite.
6. **Effort:** S.
7. **PRD fit:** §9 rubric is intentionally genre-agnostic; this adapts emphasis without
   changing guarantees. No fabrication, local-first.

---

## Special-focus evaluations (summary answers)

- **YAMNet in Rust:** feasible — `ort` crate + 16 MB Apache-2.0 ONNX from
  `zeropointnine/yamnet-onnx`, input = the exact 16 kHz WAV we already produce; windowed
  inference; runtime confirmed torch-free in podcli. Cost: one real dependency + model
  download + compile time. `tract-onnx` is the zero-native-dep fallback but slower.
  Python is out (repo is Rust-native).
- **Model-free alternative captures most value?** Yes, ~80% for typical podcasts: reaction
  anchors from energy-burst-after-sentence + laugh tokens + Q&A turns (Feature 2) reuse
  the same anchor/extension machinery, and Feature 1 slots in later as a pure upgrade.
- **Dedupe / backwards extension:** dedupe exists (55% in `mod.rs:333`, 30% in
  `validate.rs:17,172`); podcli uses ">50% of the *shorter* clip" and AYSG ">50% of the
  candidate" — our thresholds are already stricter, no change needed. Backwards extension
  is genuinely missing and is the highest-value piece of the YAMNet story; both Features 1
  and 2 implement it with the same 8 s look-back cap (FunnyNet-W evidence).
- **Hook-quality scoring for headlines:** the deterministic layer has hook detection
  (`heuristic.rs:201`, `opening_strength` at `heuristic.rs:250-258`) but there is no
  cross-check that the *LLM's* `opening_strength` matches the first ~3 s of the actual
  excerpt. Cheap add: in `validate.rs`, re-score a candidate's first 3 s with the same cue
  logic (question / HOOK_STARTS / absolute claim) and flag a mismatch
  (`opening_strength ≥ 4 while hook cues absent`) as a reason to downgrade, not reject.
  Folded into Feature 6 as a note; standalone it is ~30 lines.
- **'Exactly N' enforcement:** reject the yt-short-clipper version (forces padding;
  anti-PRD). Keep ask-more-then-prune (we already do: `plan_counts` `mod.rs:19`), add
  schema/self-check (Feature 6), and let coverage top-up (Feature 5) fix sparse regions.
- **Multi-pass selection:** podcli's bucket+coverage+merge is strictly better than our
  single fixed-window pass; Features 4 and 5 bring us to parity, and Feature 7 conditions
  the prompt the way AYSG does.

---

## TODAY — one-day, unit-testable, ranked by value

All five below are pure-Rust, zero new dependencies, and testable with the existing
fixture patterns (`validate.rs` transcripts, `heuristic.rs` `transcript_from`,
`evals/fixtures/editorial_cases.json`). No API key needed to verify.

| # | Feature | Effort | Why this order |
|---|---|---|---|
| 1 | **F3 Boundary tightening** (trim weak openings, sentence-end snap) | S | Most visible quality bug (clips opening "So, um…" / cutting mid-thought); validator is final authority, fully unit-testable, zero risk. |
| 2 | **F2 Pseudo-reaction anchors** (energy/laugh/Q&A anchors + backward extension ≤8 s) | S | Model-free 80% of the deferred YAMNet value; builds the anchor machinery Feature 1 later upgrades in place. |
| 3 | **F5 Coverage-aware top-up** (region gap detection + one targeted pass) | S/M | Kills the "all clips from one hot region" failure on the LLM path; coverage calc is pure and testable (the extra LLM call is optional at runtime). |
| 4 | **F6-lite parse hardening** (reject missing scores; integer-enforcement; prompt self-check) | S | Makes LLM proposals honest without any new calls; parser is already heavily unit-tested. |
| 5 | **F7 Genre pre-pass** | S | One cheap call; prompt-interpolation logic testable, degrade path testable (call itself needs a key — acceptable). |

Day-2 candidates (need an API key or a new dep to fully verify): **F4 global re-ranking**
(M — the PRD's missing final pass; prompt/merge plumbing is unit-testable, the HTTP leg
needs a key), **F1 YAMNet reactions** (L — start with the interface + synthetic-profile
tests, then the `ort` spike in a branch; do not merge until onnxruntime builds clean on
this machine's arm64 toolchain).

---

## Evidence ledger (key file:line citations)

- YAMNet mechanics: podcli `audio_events.py:43-49,89-95,156-174,221-246`; `plans/moment-detection.md:24,50-56,53,71-76,79-84`.
- Anchor prompting: podcli `claude_suggest.py:64-75`; blend weights `:782-783`.
- Buckets/coverage: podcli `claude_suggest.py:253-271,853-960`; dedupe `:273-290`.
- Boundary trimming: podcli `clip_generator.py:47-53,55-133,331-363`.
- Prompt contracts: yt-short-clipper `clipper_core.py:404-586` (exact-N :415, virality rubric :511-550, self-check :556, ask+3 :2295); autoshorts `llm.rs:18-60,390-470`; AYSG `highlights.py:36-62,127-162,163-171,179-197,200-249,251-270`.
- Ours: `select/mod.rs:19,37,139,186,224,284,333`; `heuristic.rs:126-127,131,201,364-376,435-441`; `validate.rs:11-12,17,19,97-117,172,196,210,230`; `energy.rs:39,179`; `openai.rs:16-47,60-77`; `pipeline.rs:444,448,499-504`; `store.rs:52-59`; `transcribe.rs:27-30`.

## Residual risks / open questions

- `ort`/onnxruntime compile & runtime behavior on this machine's Apple Silicon toolchain is
  unverified (branch spike required before committing to Feature 1).
- YAMNet inference wall-time on multi-hour podcasts is unmeasured; may need a
  settings/checkbox gate if it delays the pipeline noticeably.
- Whisper output sometimes transcribes laughter as `[Laughter]` or word fragments; laugh-token
  detection (Feature 2) needs a fixture pass against real transcripts before trusting it.
- Whether the LLM path should gain the hook-vs-score mismatch downgrade (see special-focus)
  — 30 lines, but it changes validator behavior; validate the 80% acceptance criterion
  (§16.2) before and after.
- All LLM-path features depend on provider availability; every one must degrade to
  today's behavior on any failure (all designs above do).
