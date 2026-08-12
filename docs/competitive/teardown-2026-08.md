# Competitive teardown — podcast/vertical clipping OSS (2026-08-12)

Method: cloned and read the source of the four most relevant open-source
projects found via GitHub search (live API, sorted by stars, freshness
checked via `pushed_at`). Goal: extract techniques worth folding into
Clipping Factory. Our project is MIT-licensed and these are all OSS
(MIT/AGPL/Apache), so reuse is attribution-level, not licensing risk.

## Repos analyzed

| Repo | Stars | Lang | Pushed | Relevance |
|---|---|---|---|---|
| [Anil-matcha/AI-Youtube-Shorts-Generator](https://github.com/Anil-matcha/AI-Youtube-Shorts-Generator) | 4.5k | Python | 2026-07 | OpusClip alternative; the "virality framework" |
| [jipraks/yt-short-clipper](https://github.com/jipraks/yt-short-clipper) | 936 | Python | 2026-07 | Strict prompt engineering + GPU/CPU encoder fallback |
| [JayWebtech/autoshorts](https://github.com/JayWebtech/autoshorts) | 725 | Rust/Tauri | 2026-08 | Local-first desktop; JSON-schema LLM ranking |
| [nmbrthirteen/podcli](https://github.com/nmbrthirteen/podcli) | 46 | Python | 2026-08-12 | Direct competitor: face tracking + burned captions + YAMNet |

## Techniques worth stealing (ranked)

### 1. Reaction-aware selection via audio events (podcli) — HIGH value, HIGH effort
YAMNet (AudioSet, 521 classes) as a self-contained ONNX graph via
`onnxruntime` (no torch/tensorflow). Laughter/cheering/screaming channels
collapse into reaction scores; a laugh is the strongest language-independent
"something funny happened" signal, and it anti-correlates with speech at the
frame level, so podcli uses it as an anchor and **extends clips backwards** to
the moment that caused the reaction. Degrades gracefully (feature turns off
when the model is missing).

Our status: **implemented the cheap 80%** — per-second RMS loudness
z-scoring (`src/energy.rs`), measured from the existing 16 kHz WAV via ffmpeg
`astats` (no new dependency), boosting the heuristic composite +0..4 for
loud windows. YAMNet classification (with backwards extension) is the
documented upgrade path (`ponytail:` note in `energy.rs`).

### 2. Z-score loudness normalization (podcli) — HIGH value, LOW effort
`z_avg * 0.4 + z_peak * 0.6`, mapped to 0–10, adapted to each episode's own
baseline. We do the same normalization in `energy::window_boost`.

### 3. Hardware video encoding with fallback (yt-short-clipper) — HIGH value, LOW effort
GPU encode first, runtime-detect failure and swap to CPU. We implemented the
macOS equivalent: `h264_videotoolbox` probed once per process from
`ffmpeg -encoders`, fallback to `libx264 veryfast crf 19` (`render.rs`).
~3–5× faster renders on Apple Silicon, visually equivalent H.264.

### 4. Strict LLM prompt contracts (yt-short-clipper, autoshorts) — MEDIUM value, LOW effort
- yt-short-clipper: exactly N clips (never fewer), hard 60–120s window,
  priority ladder: conflict/tension/controversy > personal admissions > sharp
  statements > punchlines > complete stories > standalone hooks; "when in
  doubt, prefer EMOTION & CONFLICT over neutral education."
- autoshorts: JSON-schema-enforced structured output (`format` field), 30–90s,
  temperature 0.2, viral-strategist system prompt, requires 3–10 candidates.

Our status: **implemented** — the priority ladder + hook-first-3-seconds +
energy/emotion guidance is now in `SYSTEM_PROMPT` (`select/mod.rs`). We keep
our harder guarantees: verbatim quotes, honest 1–5 scoring, and the validator
can still reject anything the prompt oversells.

### 5. Speaker-aware crop with stable runs (podcli) — MEDIUM value, HIGH effort
`video_processor.py` (3.3k lines): split-screen detection; face tracks split
into "stable runs" (jump threshold = 22% of crop width, gap threshold 0.55 s,
candidate probing before committing a split); per-run representative center
scored by distance from a seed (72% first + 28% median) plus an edge-margin
penalty (keeps crops off frame edges); boundary runs ≤1.4 s that hug the edge
are merged into more-central neighbors.

Our status: frame.rs has a single persistent cluster with smoothed,
clamped forward-fill. Run-splitting + boundary trim would help multi-speaker
episodes. **Deferred deliberately** — framing is the user-sensitive surface
(the zoom-out was reverted twice); revisit only with user sign-off.

### 6. Caption styles (podcli) — MEDIUM value, MEDIUM effort
"hormozi": 2–3 words at a time, active-word color pop, uppercase, box behind
active word, margin_v 180. "karaoke": full sentence visible with progressive
word highlight. Font detection via `fc-list` fallback chain.

Our status: impact/clean styles + presets + accent colors. A karaoke-style
word-highlight mode is the natural next caption feature. Not yet implemented.

### 7. Misc
- autoshorts: multi-provider abstraction (DeepSeek/Claude/Gemini/OpenAI/
  Groq/local Ollama) behind one prompt — we already abstract OpenAI/Anthropic/
  offline the same way. No action.
- AYSG: >30 min videos auto-chunked with overlap; overlapping highlights
  collapsed by score. We chunk into transcript windows and enforce overlap
  containment in the validator. No action.
- podcli: MCP server so agents can drive the CLI. Our studio has a JSON API;
  an MCP adapter is a possible future product surface.
- AYSG/yt-short-clipper: `--output-json` automation surface. We expose JSON
  through `/api/projects/{id}`. No action.

## What we deliberately did NOT copy
- Anything that fabricates or embellishes content (hooks appended by LLM
  without source text, rewritten headlines) — violates the PRD's
  no-slop contract.
- Cloud-only pipelines (MuAPI/Deepgram) — we stay local-first; the live
  studio runs entirely on the user's machine.

## Next candidates
1. YAMNet reaction detection with backwards clip extension (needs ~15 MB
   ONNX model + onnxruntime; optional feature that self-disables).
2. Karaoke/word-highlight caption style.
3. podcli-style stable-run crop splitting (with user sign-off).
