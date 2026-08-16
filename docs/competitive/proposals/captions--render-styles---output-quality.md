# Captions, render styles & output quality — competitive proposals

Date: 2026-08-12
Scope (case-init): competitive teardown, feature area = **captions, render
styles & output quality** of our app (Clipping Factory, Rust + axum + ffmpeg +
whisper.cpp, `~/.clipping-factory` local-first studio).

Competitors read (read-only): `podcli` (Python, direct competitor) —
`backend/config/caption_styles.py`, `services/caption_renderer.py`,
`services/captions_burn.py`, `services/encoder.py`,
`services/thumbnail_generator.py`, `services/thumbnail_ai.py`;
`yt-short-clipper` — `clipper_core.py` (hook burn, fonts, caption ASS);
`autoshorts` — `src-tauri/src/media.rs` (cropping only — **out of scope**,
framing is user-sensitive per constraint; no caption/overlay code exists
there: it renders a raw `crop=` filter, media.rs:133-135).

AYSG (4.5k stars) checked via GitHub tree API:
`AI-Youtube-Shorts-Generator/shorts_generator/` contains only
clipper/downloader/highlights/transcriber/llm — **no caption, subtitle, ASS,
hook, or overlay code at all** (tree grep returned zero matches). It produces
captionless clips. Finding: our caption system already exceeds AYSG; nothing
to steal there for this area.

Methodology: read only the modules mapping to the feature area; every finding
cites `file:line`; every Finding ends with a Path for our codebase; no
competitor source was modified. Existing teardown
`docs/competitive/teardown-2026-08.md` covers selection, energy, encoder
fallback, framing, and the *existence* of podcli caption styles (its item #6);
this proposal is the detailed design that teardown deferred
("A karaoke-style word-highlight mode is the natural next caption feature" —
teardown-2026-08.md, "Caption styles" row, plus "Next candidates #2").

Constraint honored: nothing below changes crop/pan/framing unless behind a
new opt-in control. No new runtime services. Zero or tiny new dependencies.
No-slop contract: nothing here fabricates speech, hooks, or imagery; every
text element is either the verified headline (PRD §9.4) or verbatim
transcript words (PRD §10), and every graphic is user-supplied or a real
source frame.

---

## Feature 1 — Karaoke caption style (progressive word highlight) ★ deferred item

### 1. What it does (user-visible)
A third caption style in the per-clip restyle panel: the full sentence
(3–7 word group) stays visible on screen and the **currently spoken word
progressively highlights** — a smooth color sweep (karaoke fill) from
inactive white to the user's accent color, word by word, exactly synced to
the whisper word timestamps. No page pop, no kinetic scaling — the restrained
PRD §11.3 treatment, upgraded with the "sing-along" word timing that short-form
viewers read as high polish.

### 2. Competitor evidence
- podcli style definition `backend/config/caption_styles.py:79-102` —
  `"karaoke"`: `font_size 60`, inactive `primary_color &H00808080` (gray),
  active `&H00FFFFFF` (white), `words_per_chunk 5`, alignment 2, `margin_v 160`.
- podcli renderer `backend/services/caption_renderer.py:229-290`
  `_render_karaoke` — one Dialogue **per chunk** (not per word), text built as
  `{\c<active>}{\2c<inactive>}` + `{\kf<duration_cs>}word` joined by spaces;
  `\kf` = libass karaoke-fill tag (smooth sweep; `\k` would be a hard sweep).
  This is the proven-on-ffmpeg/libass mechanism.
- podcli chunking `caption_renderer.py:147-173` `_chunk_words` — chunk breaks
  after `words_per_chunk`, after terminal punctuation, on speaker change, or
  across a gap > `CHUNK_BREAK_GAP = 0.8s` (line 143); plus
  `_hold_through_gap` (lines 175-183, cap `CAPTION_GAP_FILL_MAX = 0.4s`) so a
  pause on a chunk boundary never blanks the screen. Their tests pin this:
  `tests/test_caption_renderer.py:30-90` (splits on word count, punctuation,
  long gap; short gap does NOT break; speaker change breaks).
- Our own gap in the teardown explicitly names karaoke as next
  (`docs/competitive/teardown-2026-08.md`, "Next candidates #2").

### 3. Implementation sketch (our codebase)
All in `src/captions.rs` — no new dependencies, no render.rs change
(captions are already burned from an ASS file in the second pass).

1. `pub enum CaptionStyle` (captions.rs:13) gains `Karaoke`; update
   `from_str` (19), `parse_strict` (26) — **strict parse must accept
   `"karaoke"`**, `label()` (33), `default_accent_hex` (62).
2. `pub fn build_ass` (155) dispatches `CaptionStyle::Karaoke =>
   build_karaoke(input)`.
3. New `fn build_karaoke(input: &CaptionInput) -> String`:
   - Reuse `paginate()` (captions.rs:629) — it already breaks on punctuation,
     width (30 chars) and pauses (≥700 ms), giving 3–7 word groups = the
     karaoke sentence.
   - Reuse `relative_words()` (captions.rs:162) for zero-length-word repair and
     clip-relative timing (repair loop at captions.rs:177-178).
   - One Dialogue per chunk: `start = chunk[0].start_ms`,
     `end = next chunk start` (hold-through-gap cap 400 ms — Feature 7 formalizes
     this helper so both styles share it).
   - Text per chunk: `{\c&H{accent}&}{\2c&HFFFFFF&}` + for each word
     `{\kf{dur_cs}}{text}` joined with spaces; `dur_cs` =
     `(end_ms - start_ms)/10` clamped ≥ 1 (reuse `escape()`, captions.rs:703,
     for text).
   - New header style `Karaoke` (mirror `clean_header` captions.rs:574):
     font = chosen caption font, `66` px, `&H00FFFFFF`, outline 3.4, shadow 1.2,
     BorderStyle 1, alignment 2, MarginV 400. **No `\an5`/`\pos`, no `\t` pop** —
     karaoke must rely on libass layout so the fill sweeps naturally.
4. Wire-through (no behavior change elsewhere):
   - `src/api.rs` `restyle_clip` (638) — the strict parse at 691-694 already
     flows to `build_ass`; the error string "style must be \"impact\" or
     \"clean\"" (693) needs "karaoke" added.
   - `src/pipeline.rs:683-765` — `CaptionStyle::from_str` (684) picks up
     karaoke automatically; no pipeline change.
   - `static/app.js` `restyleControls` (646) — `styleBtns` array at app.js:675
     (`["impact", "clean"]`) gains `"karaoke"`; nothing else in the restyle
     flow (dirty-check, payload, apply) needs to change.
5. Tests (same module, `#[cfg(test)]`): one Dialogue per chunk; `\kf`
   durations per chunk sum to the chunk span; every word appears exactly once;
   no two chunk windows partially overlap (reuse `parse_events` helpers at
   captions.rs:810); accent appears only in the `\c` fill tag; zero-length
   words still get a fill slot; edited caption text via `with_caption_text`
   (captions.rs:81) keeps karaoke timing coherent.

### 4. Dependencies / risks
- Zero new dependencies. `\kf` is standard libass, already shipped by the
  ffmpeg builds we require (the same `ass=` filter our burn pass uses).
- Risk: a very short word (< 10 ms after repair) yields `dur_cs = 0` — clamp
  to 1 cs (libass tolerates 0, but 1 reads cleaner).
- Risk: `\kf` + `WrapStyle 2` wraps mid-word on long chunks; the existing
  30-char page budget keeps chunks ≤ 2 lines. Accept.

### 5. Effort
**S** (one file + 5 tests; UI is one array element).

### 6. PRD fit
§11.3 "captions display short conversational groups … the currently spoken
word may use one restrained accent color" — karaoke is exactly this, with the
accent already enforced by our `accent_bgr_for` pipeline. §6.4 restrained
treatment: no motion added beyond the word sweep. No-slop: text is verbatim
transcript words. Local-first: pure Rust string building.

---

## Feature 2 — Hormozi word-chunk caption style

### 1. What it does (user-visible)
A fourth style: **2–3 words on one line, ALL CAPS, bold, active word pops in
the accent color**, sitting in the lower safe area with a soft box behind it.
The classic short-form "punchy" caption rhythm (each chunk is one phrase, not
a full sentence). Distinct from Impact (which stacks words at different sizes
with a huge emphasis word) and from Karaoke (full sentence, sweep fill).

### 2. Competitor evidence
- podcli `backend/config/caption_styles.py:59-78` — `"hormozi"`:
  `font_size 80`, bold, uppercase, `active_color` pop
  (`&H0000FFFF` yellow), `border_style 3` (opaque box using `back_color`
  `&H80000000`), `words_per_chunk 3`, `margin_v 180`.
- podcli renderer `caption_renderer.py:187-227` `_render_hormozi` — chunked
  like karaoke, one Dialogue per chunk, `\kf` fill with `\c` active / `\2c`
  inactive, uppercase applied per word (line ~211).

### 3. Implementation sketch
Shares 95% of Feature 1's machinery; the differences are parameters, so build
them together (one builder, two param sets) rather than two codepaths:

- `fn build_wordchunk(input, params)` where params carry
  `chunk_size`, `uppercase`, header style block.
- `CaptionStyle::Hormozi`:
  - chunk size **3** (podcli parity; our `paginate()` caps at 7 words —
    introduce a `chunk_size` parameter on the paginator, default 7; hormozi
    passes 3; karaoke passes 5-7).
  - `uppercase = true` (unlike karaoke).
  - Header style `Hormozi`: font 74 bold, BorderStyle **3** with
    `BackColour &H80000000` semi-transparent box, alignment 2, MarginV 220.
  - Same `\kf` fill mechanics as Feature 1 (or, simpler and already proven by
    our Clean style, per-word `\c` swap events — both acceptable; `\kf` keeps
    the sweep consistent with karaoke).
- Enum/wiring: same edits as Feature 1 (captions.rs:13-33, api.rs:691-694,
    app.js styleBtns at app.js:675) — plus `parse_strict("hormozi")`.
- Tests: chunk ≤ 3 words; uppercase applied; box style present in header;
  accent flows into the active tag; same window-non-overlap assertions.

### 4. Dependencies / risks
- Zero new deps. Risk: 3-word chunks change rhythm vs PRD §11.3's "3–7 words"
  — hormozi's 3 is at the low edge of that band; keep 3 (2 feels choppy) and
  document the choice. Risk: uppercase on long acronym words (already handled
  by keeping verbatim transcript text, casing only). No-slop unaffected
  (casing normalization is PRD §10-sanctioned).

### 5. Effort
**S** (piggybacks Feature 1's builder; separate tests).

### 6. PRD fit
§6.4 "one restrained house style" — hormozi stays on the curated font +
accent pipeline, no decorative elements; §11.3 high-contrast lower-safe-area
captions. Local-first, no deps.

---

## Feature 3 — Caption position, margin & font-scale controls

### 1. What it does (user-visible)
Per-clip restyle gains a **Position** control (Lower / Center / Upper) and a
**Size** slider (60–160%) for captions. Solves the real-world complaint:
platform UI (Like/Share buttons, "part 2" overlays, TikTok handles) sits at
the bottom and covers the caption line; users can lift captions out of the
way without touching framing.

### 2. Competitor evidence
- podcli `services/caption_renderer.py:96-123` — `render_captions(..., caption_position, caption_font_scale)`;
  `position_margins = {"upper": 760, "center": 480, "lower": 220}` (line 121)
  map onto ASS `MarginV`; `scale = max(60, min(160, ...))/100` (line 119)
  multiplies `font_size` before header emission.
- autoshorts media.rs:133-135 — no caption concept; confirms position is our
  own surface, not a copy.

### 3. Implementation sketch
- `src/captions.rs`:
  - New `pub enum CaptionPosition { Lower, Center, Upper }` +
    `pub fn margin_v_for(style, pos) -> u64` (Lower 220, Center 480, Upper 760
    — podcli parity) used by `clean_header`/`karaoke`/`hormozi` styles via a
    new `position` field on `CaptionInput` (captions.rs:124).
  - Impact uses explicit `\pos` (captions.rs:357+), so position maps to
    `BLOCK_ANCHOR_Y` (captions.rs:203, value 1270) and the clamp band (captions.rs:204-205, 920–1640): Lower → 1450,
    Center → 1270, Upper → 1000, re-clamped in `layout_lockup` (captions.rs:270).
  - New `pub fn scaled_font_size(base: u32, scale_pct: u16) -> u32` clamp
    60–160, applied at header build.
- `src/api.rs` `RestyleIn` (609) gains `position: Option<String>` and
  `font_scale: Option<u16>`; `restyle_clip` (638) validates and persists them
  on the clip; `src/domain.rs` clip struct gains the two fields (project JSON
  is additive — old projects default to Lower/100).
- `src/pipeline.rs:683-765` passes clip position/scale into `CaptionInput`.
- `static/app.js` `restyleControls` (646): add a 3-button `seg` for position
  and a range input for size; include both in the dirty-check and the
  restyle payload (~line 812).
- Tests: margin mapping table (pure fn); scale clamp; impact anchor remap stays
  inside the safe band; old-project default = Lower/100.

### 4. Dependencies / risks
- Zero deps. Risk: moving captions Up over a face — mitigated because Upper
  (760) is still below the top third where faces crop; no framing change, so
  the constraint ("no crop changes") is untouched. Risk: headline overlay
  (clean style, MarginV 110) and upper captions could collide on very short
  clips — skip headline when position = Upper.

### 5. Effort
**M** (captions.rs + api.rs + domain.rs + app.js; all unit-testable except
the UI wiring).

### 6. PRD fit
§11.3 high-contrast captions in the lower safe area = the default (Lower);
position is an explicit user override, not a default change. §19.4
"post-MVP: user-controlled caption customization" — this is the first slice
of it, done locally with zero services.

---

## Feature 4 — Automatic font fallback detection

### 1. What it does (user-visible)
The font dropdown shows **only fonts actually usable on this machine** (with
bundled Inter always guaranteed), and if a clip was styled with a font that
is no longer installed, restyle/build transparently substitutes the closest
available face instead of letting libass silently fall back to an ugly
system default. No "why did my captions change fonts?" mysteries.

### 2. Competitor evidence
- podcli `backend/config/caption_styles.py:12-53` `_detect_font()` — runs
  `fc-list --format=%{family}\n`, parses comma-separated family lists
  (line 42), picks the first candidate present from
  Arial → Helvetica → Liberation Sans → Noto Sans → DejaVu Sans → FreeSans →
  `sans-serif`, falling back to `candidates[0]` (Arial) if fontconfig is
  missing (macOS without fontconfig).
- podcli `caption_renderer.py:397-446` `_measure_text_widths` — uses
  `fc-match` to resolve the *exact* font libass will use before pixel
  positioning (evidence that font-resolution mismatches are a real problem
  they engineered around).
- yt-short-clipper `clipper_core.py:361-406` — per-platform hardcoded font
  paths (`darwin` → `/System/Library/Fonts/Supplemental/Arial Bold.ttf`,
  `/Library/Fonts/...`; fallback `font='Arial'`), the non-fontconfig
  alternative that macOS needs.

### 3. Implementation sketch
- New `src/fonts.rs` (small; or fold into captions.rs — prefer separate for
  probe caching):
  - `pub fn installed_caption_fonts() -> Vec<String>` — lazy static; tries
    `fc-list --format=%{family}\n` (timeout 2 s, ignore failure), parses
    `"Family1,Family2"` lines into a set; on any failure (macOS usually has
    no fontconfig), falls back to probing known paths
    (`/System/Library/Fonts`, `/System/Library/Fonts/Supplemental`,
    `/Library/Fonts`, `~/Library/Fonts`) for `Inter*`, `Arial*`,
    `Helvetica Neue*`, `Avenir Next*`, `Verdana`, `Georgia`.
  - Intersect with `CAPTION_FONTS` (captions.rs:44) and always append
    `"Inter"` (bundled `assets/fonts/Inter-*.ttf` via `CF_FONTS_DIR`,
    config.rs:103, passed through `fontsdir=` in render.rs:235) as the
    guaranteed floor.
  - `pub fn resolve_caption_font(selected: &str) -> &'static str` — selected
    available → selected; else `"Inter"` (with a `was_substituted` note the
    UI can show once).
- `src/api.rs` settings (151) — `caption_fonts` becomes the live-detected
  list; `restyle_clip` font validation (618, and the existing
  `caption_font_name` guard at 644) now validates against the live list with
  substitution, never rejects outright.
- `static/app.js` (106-107) already consumes `caption_fonts`; add a one-time
  notice when a stored font was substituted.
- Tests: `fc-list` output parsing (fixture string "Arial,Helvetica\nInter");
  resolver substitution table; path-probe logic as a pure filter fn over a
  fake dir listing. Real-machine probe = manual smoke test only.

### 4. Dependencies / risks
- Zero new deps (one `fc-list`/path subprocess probe, cached once per
  process). Risk: fontconfig absent → path-probe heuristics can't enumerate
  every font; acceptable because the UI only *suggests* — the curated list +
  bundled Inter floor means any choice still renders. Risk: fc-list on some
  systems returns localized names — treat as best-effort, never fatal.

### 5. Effort
**S–M** (probe + resolver + API wiring + UI notice).

### 6. PRD fit
§11.3 "one readable sans-serif typeface" — this *enforces* it (never a silent
fallback to a random face). §16.2 caption quality gate. Local, zero deps,
no services.

---

## Feature 5 — Hook card overlay (opt-in, default OFF) — no-slop compliant

### 1. What it does (user-visible)
An optional **text hook card** at the very start of a clip: for the first
~2 seconds the *verified headline* (or, if the headline duplicates the
opening words, the clip's first 3–5 spoken words verbatim) appears large in
the upper third, then normal captions proceed. **Default off.** No TTS voice,
no frozen frame, no invented text — it is a text card on real footage using
words the speaker actually says.

### 2. Competitor evidence
- yt-short-clipper `clipper_core.py:3149-3260` `add_hook` — the full hook
  pattern (TTS narration + freeze-first-frame + bold yellow 3-words-per-line
  text at the upper third, `drawtext` box=1 white@0.95 boxcolor, line 3250+).
  Their hook **fabricates speech** (TTS of user-supplied text) — we copy the
  *text-card layout*, not the fabrication.
- PRD §9.4 already sanctions headline display ("may appear as restrained
  top-of-frame text") — our Clean style already has a 3.5 s headline overlay
  (captions.rs:540 `show_headline`, Dialogue emitted at 541-549); this feature
  upgrades it to an explicit, restylable, opt-in control.

### 3. Implementation sketch
- `src/captions.rs`: new optional `hook: Option<HookSpec>` on `CaptionInput`
  (captions.rs:124); new header style `Hook` (54 px bold, white, `BorderStyle 1`,
  alignment **8** = top-center, margin_t 120); in `build_clean`/`build_*`
  (only when `hook` is Some): emit one Dialogue `0 → min(2000, clip_len)` with
  `{\an8}` text = headline if `show_headline(headline, words)` (captions.rs:664) is true,
  else first 3–5 verbatim words joined; **skip headline emission** (call site captions.rs:540)
  when the hook card is active (avoid double top text).
- `src/api.rs` `RestyleIn` (609) gains `hook: Option<bool>` (default false =
  current behavior preserved); persisted on the clip; `src/domain.rs` additive
  field defaulting false.
- `static/app.js` `restyleControls` (646): a checkbox "Opening hook card"
  included in dirty-check + payload (~812).
- `src/config.rs`: optional `CF_HOOK_DEFAULT` env for project-level default;
  still defaults to off.
- Tests: hook emitted only when Some; hook text is never empty; hook uses
  headline only when `show_headline` passes, else verbatim opening words
  (assert the exact source words appear in output); hook duration ≤ 2 s;
  no hook when `clip_len < 2500 ms`; headline double-emission suppressed.

### 4. Dependencies / risks
- Zero deps, no TTS, no network. Risk: a headline could still contain a
  summary word not spoken verbatim — PRD §9.4 says headline "must be supported
  directly by what the speaker says" and the validator already checks quotes;
  the card renders the *headline text as a hook*, which is within §9.4's
  "top-of-frame text" sanction. Flag for product sign-off that the card makes
  the headline more prominent than §9.4's "restrained".
- Explicitly NOT: yt-short-clipper's TTS hook (would violate no-slop).

### 5. Effort
**S–M** (ASS event + API field + checkbox; unit-testable).

### 6. PRD fit
§6.4 "no visual element added only to create motion" — hook is opt-in and
text-only; §9.4 headline rules honored; default off preserves current output
exactly.

---

## Feature 6 — Brand logo overlay + optional end card (user-provided brand)

### 1. What it does (user-visible)
A settings toggle lets the user supply **their own logo image** (PNG with
alpha), burned at small size into the corner (default top-right, opacity
slider), and optionally append a 0.8 s **end card**: the clip's *real last
frame* frozen, with the verified headline centered, fading out. Default:
off — current output unchanged.

### 2. Competitor evidence
- podcli `services/captions_burn.py:57-127` `burn_captions` — logo overlay
  chain: `scale=-1:{logo_height}[logo]` then
  `[base][logo]overlay={x}:{y}` with margin/position args (lines 98-111), plus
  a gradient PNG via lavfi `geq` (lines 25-55) for caption legibility; burned
  in the same pass as captions.
- podcli `services/thumbnail_generator.py:794-822` `thumbnail_to_video_frame` —
  loop a PNG → 0.8 s clip with fade-in/out, appended after the video.
- yt-short-clipper `clipper_core.py:4571-4650` — watermark overlay with
  scale/position/opacity via `colorchannelmixer=aa={opacity}` (line ~4620).

### 3. Implementation sketch
- `src/render.rs` `burn_captions` (58): add optional `logo: Option<LogoSpec>`
  (path, position, height_px, opacity) and `end_card: Option<EndCardSpec>`
  (frame path, headline, duration). Build the filter chain like podcli
  captions_burn.py:98-111 — second input `-i logo.png`, `scale=-1:{h}`,
  `format=rgba`, `colorchannelmixer=aa={opacity}`, `overlay=x:y` — then the
  existing `ass=` step (subtitles_filter, render.rs:235). Escaping: reuse
  `ff_escape_str` (render.rs:322) for logo path.
- End card as a separate mini-render: extract last frame once per clip
  (`ffmpeg -ss {end-0.1} -i base -frames:v 1`), render `thumbnail_to_video_frame`
  equivalent (loop + fade, podcli thumbnail_generator.py:794-822), then
  concat via `-f concat` with identical encode args (`video_encode_args`,
  render.rs:179) — both segments use the same encoder/params so concat is
  clean; audio: end card is silent (`-an`), pad the original audio to match.
- Settings: `src/settings.rs`/`src/config.rs` project-level `brand` block
  (logo path, position, opacity, end_card toggle) + UI in `static/app.js`
  (upload logo into the project; toggles in the restyle panel — end card is
  per-clip, logo is per-project).
- Tests: filter-string construction for logo chain (unit), position math,
  opacity clamp; end-card render command args. Visual QA required for the
  concat seam (same-day unit tests, next-day visual sign-off).

### 4. Dependencies / risks
- Zero new deps (pure ffmpeg filter). Risk: concat requires identical
  codec/resolution/params — mitigated by reusing `video_encode_args` for both
  segments and asserting ffprobe equality in a test helper. Risk: burned logo
  is permanent (not removable) — acceptable for brand overlays, matches
  competitors. No-slop: logo is user-provided; end-card frame is a real
  source frame; text is the verified headline.

### 5. Effort
**M–L** (render.rs filter chain + end-card concat + settings UI + QA).

### 6. PRD fit
§6.4 restrained treatment (small corner logo, opt-in); end card reuses real
content only. Local, zero services. This is the closest thing to "output
quality" packaging in the list; mark as post-MVP §19.4 territory if the
owner prefers to keep the MVP canvas clean.

---

## Feature 7 — Caption timing polish: hold-through-gap + pause breaks

### 1. What it does (user-visible)
Small but felt: on a pause inside a phrase, the current caption **stays up
instead of vanishing mid-thought** (hold-through-gap, capped at 400 ms), and
a long pause (≥ 0.8 s) forces a clean new chunk. Reduces the "flashing
captions" feel on podcasts with natural rhythm.

### 2. Competitor evidence
- podcli `caption_renderer.py:142-143` — `CAPTION_GAP_FILL_MAX = 0.4`,
  `CHUNK_BREAK_GAP = 0.8`; `_hold_through_gap` (175-183) extends a chunk's
  end toward the next chunk's start, capped, never overlapping; `_chunk_words`
  (147-173) breaks on gap > 0.8 s. Tests pin it:
  `tests/test_caption_renderer.py:46-55` (long gap breaks, short gap doesn't).
- Our current state: `build_clean` already keeps a neutral line through the
  inter-word gap (captions.rs:566-571) but hard-caps page end at
  `last.end + 160 ms` (captions.rs:556-558); `paginate` breaks at ≥ 700 ms
  (`PAGE_GAP_MS` captions.rs:531, pause check at 650); impact at ≥ 600 ms with
  +200 ms hold (captions.rs:374). Parity is close; the polish is *consistency* + the 400 ms cap rule.

### 3. Implementation sketch
- `src/captions.rs`: extract `fn hold_through_gap(chunks, idx, end_ms,
  next_start) -> u64` (cap `min(next_start, end + 400)`, never overlap) used
  by `build_clean`, `build_karaoke`, `build_wordchunk`; keep impact's
  existing +200 ms behavior but route it through the same helper so all
  styles share one rule. Normalize break-gap constants
  (`const PAGE_GAP_MS` clean 700 / impact 600 → document vs podcli 800; keep
  ours — they're tuned to whisper's timing).
- `_sanitize_words` parity (podcli caption_renderer.py:64-91): our
  `relative_words` already repairs zero-length words (captions.rs:177-178);
  add the ≥ 50 ms minimum-duration rule for karaoke `\kf` durations.
- Tests: hold-through-gap caps at 400 ms and never overlaps next chunk;
  short gap (< 400 ms) holds; long gap (≥ 800 ms) breaks; all three styles
  share the helper (assert identical windows for identical input).

### 4. Dependencies / risks
- Zero deps. Risk: over-holding a caption into a sentence start (mitigated by
  the 400 ms cap and never-overlap rule — both unit-tested).

### 5. Effort
**S** (one helper + three call sites + tests).

### 6. PRD fit
§11.3 consistent captions; §16.2 timing drift gate. Pure behavior polish,
zero risk to content.

---

## TODAY — single-day implementable + testable, ranked by value

All of these are pure-Rust, unit-testable without real video, and touch only
`src/captions.rs` (+ one array in `app.js` for style buttons); none require
render QA to merge safely (they are exercised by the existing `cargo test`
caption suite).

1. **Feature 1 — Karaoke style** (the deferred teardown item; highest
   visible value, one enum variant + one builder + `\kf` mechanics + 5
   tests). Do first; Features 2 and 7 build on its chunk/hold plumbing.
2. **Feature 7 — Hold-through-gap + pause-break parity** (shared helper the
   karaoke builder needs anyway; also improves existing clean/impact output
   immediately).
3. **Feature 2 — Hormozi word-chunk style** (a parameter set on the Feature 1
   builder; ~30 lines + tests once karaoke lands).
4. **Feature 5 — Hook card (opt-in)** (new ASS style + one API field + one
   checkbox; unit-testable; default-off keeps outputs identical).
5. **Feature 3 — Position / size controls** (captions.rs margin/scale fns +
   api.rs/domain.rs/app.js wiring; UI smoke-test needed, logic is unit-tested).
6. **Feature 4 — Font fallback detection** (probe + resolver unit-tested
   against fixtures; the machine-level probe is a manual smoke check).

Features 6 (logo overlay + end card) is **not** a TODAY item: the concat seam
and burn quality need visual QA on real renders; it also deserves a product
decision on whether burned branding belongs in the MVP canvas
(PRD §19.4 suggests post-MVP).

Acceptance-risk note: Features 1–5 change emitted ASS text; the existing
window-non-overlap and accent-flow tests (captions.rs test module) must stay
green, and `restyle_clip`'s strict style parse (api.rs:691-694) is the single
place where unknown-style rejection is enforced for the API — add "karaoke"
and "hormozi" there or the UI buttons will 400.
