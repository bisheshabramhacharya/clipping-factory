# Clipping Factory

One podcast episode in, a few faithful vertical clips out. Local-first desktop tool: inspect → extract audio → transcribe → find candidates → validate → frame → render.

## Language

**Source**:
The uploaded podcast episode (MP4) a project is built from. Stored verbatim — never transcoded on ingest.
_Avoid_: input, original, video file

**Candidate**:
A proposed clip interval from the transcript, before validation.
_Avoid_: proposal, moment

**Clip**:
A validated excerpt of the Source that gets rendered to a vertical MP4.
_Avoid_: highlight, segment

**Base clip**:
The framed, uncaptioned intermediate MP4 kept on disk so caption restyling doesn't re-run the expensive render pass.

**Shot**:
A stretch of a Clip between two camera cuts in the Source. The camera holds still within a Shot, so face detections are pooled across it.
_Avoid_: scene, segment

**View**:
The static framing for one Shot, or for one speaker turn inside a Shot: a 9:16 crop window (position plus zoom), or BlurPad when nobody usable is in frame. A View never pans or eases; the frame hard-cuts from one View to the next (ADR-0004).
_Avoid_: locked crop, keyframe, pan, face tracking

**Active speaker**:
In a Shot with several people, the one whose mouth moves most while words are spoken. The View switches people only on turns long enough to beat the switching cost (~2 s), on the first word of the turn.
_Avoid_: dominant face, diarization

**BlurPad**:
The full Source frame centered over a blurred, darkened copy of itself. Used for a View with no usable face, and for a whole Clip under Fit framing or with no faces at all.
_Avoid_: letterbox, padded layout

**Downscale-only output**:
Output size equals the native full-height 9:16 window, capped at 1080×1920; smaller sources produce smaller output rather than stretched output. The only upscaling is a zoomed View, at most 1.6×.
_Avoid_: fit-to-canvas, normalize to 1080

**Auto-cut**:
A per-Clip opt-in that removes silence gaps and filler words ("um", "uh") at render time via a keep-list concat in the base render. Off by default — a Clip is otherwise one continuous faithful excerpt.
_Avoid_: smart trim, jump cut, edit decision

**Eye line**:
Where a zoomed View places the face: the estimated eye height (face center minus ~15% of the face-box height) lands near 36% of the output height. Full-height Views have no vertical room and stay exact crops.
_Avoid_: headroom rule, vertical tracking

**Composite score**:
The weighted sum of a Candidate's seven validator scores (self-contained ×2, payoff ×1.6, opening strength ×1.4, clarity ×1.2, tension/novelty, specificity, minus context-dependency and slop-risk), plus a duration nudge inside the Platform target's sweet-spot window (25–60s under Any). Surfaced on every Clip card and in the rejected list.
_Avoid_: virality score, AI score

**Scene guard**:
The validation rule that a Clip may not open or close inside a detected scene transition: cuts inside ±500 ms of a boundary snap to word boundaries or the Candidate is rejected. Spanning a boundary (a multi-cam camera switch) is allowed.
_Avoid_: shot detection

**Cold-open guard**:
The validation rule that a Clip may not open on a greeting, housekeeping line, or lone filler word — every Clip starts mid-thought.
_Avoid_: hook check

**Caption-only clip**:
What a Source shorter than 20 seconds becomes: one Clip spanning the whole Source, captioned and framed as usual. Selection and the validator's score and duration rules are skipped because there is nothing to choose between.
_Avoid_: full-video mode

**Zoom cuts**:
Per-Clip opt-in `zoompan` punch-ins on energy/emphasis beats over the framed canvas. Zoom rests at 1.0 outside rise/fall; retimed through Auto-cut removals.
_Avoid_: ken burns, animated crop

**Caption style**:
The per-Clip caption look — Impact (default karaoke), Clean, Pop (per-word pop + keyword accent), Cinema (lowercase letterspaced fade) — applied at caption time from the Base clip, so restyle is seconds not a re-render. In every style the accent color is only on the word being spoken: never two accented words at once.
_Avoid_: template, preset

**Export pack**:
The per-Clip share bundle: `.srt`, `.vtt`, `.meta.json` (title/description/hashtags), and a poster still beside the rendered MP4.
_Avoid_: publish, distribution


**Progress bar**:
Opt-in thin accent-colored fill strip along a Clip's bottom edge.
_Avoid_: scrubber

**Hook title**:
Opt-in ~1.8 s headline burned into a Clip's opening frames, upper third. ALL-CAPS under Impact and Pop; headline case under Clean and Cinema.
_Avoid_: title card, intro

**Focus prompt**:
Optional free-text direction ("clips about pricing") that steers LLM candidate selection; the heuristic fallback matches keywords instead.
_Avoid_: topic filter

**Platform target**:
The destination platform a project optimizes for, picked at upload (Any / TikTok / Reels / Shorts). Re-centers the duration window the Composite score's sweet-spot nudge rewards — TikTok 25–35s, Reels 35–45s, Shorts 45–60s, Any 25–60s — and adds a preference hint to the selector's window prompt. A ranking preference only: the validator's accept bounds never move.
_Avoid_: export preset, publish target

**Review**:
The pause between validation and framing, for projects uploaded with review on. The run parks at `awaiting_review` after the validator resolves the Candidates; nothing is framed or rendered until a decision posts. The server is the source of truth — a refresh or restart keeps the Review where it was.
_Avoid_: approval step, triage queue

**Kept rank**:
The rank of an accepted Candidate the user asked to render. Kept ranks accumulate on the project (`kept_ranks`) — the set only grows, so adding a rank to a finished project frames and renders just the new Candidate and never re-renders a ready Clip.
_Avoid_: approved candidate, selected clip
