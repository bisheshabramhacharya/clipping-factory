# Clipping Factory

One podcast episode in, a few faithful vertical clips out. Local-first desktop tool: inspect → extract audio → transcribe → find moments → validate → frame → render.

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

**Dominant face**:
The single face cluster that is largest/most persistent across a Clip's sampled frames. The lock target for framing.
_Avoid_: active speaker, main face

**Locked crop**:
The 9:16 crop window fixed at the Dominant face's position for the entire Clip. Never pans, never eases — the camera does not move.
_Avoid_: face-tracked crop, smoothed crop, pan (all removed concepts)

**BlurPad**:
Fallback layout: the full Source frame centered over a blurred, darkened copy of itself. Used when there is no reliable single face.
_Avoid_: letterbox, padded layout

**Opening face gate**:
The rule that a Clip must open on the Dominant face already in frame — never on an empty room, table, or transition.
_Avoid_: leading-gap guard

**Downscale-only output**:
The rule that rendered pixels are never upscaled. Output size equals the native crop window, capped at 1080×1920; smaller sources produce smaller (sharp) output rather than stretched output.
_Avoid_: upscale, fit-to-canvas, normalize to 1080

**Auto-cut**:
A per-Clip opt-in that removes silence gaps and filler words ("um", "uh") at render time via a keep-list concat in the base render. Off by default — a Clip is otherwise one continuous faithful excerpt.
_Avoid_: smart trim, jump cut, edit decision

**Diarization**:
Per-project record of who speaks when (`speakers.json`): speaker turns built from transcript speech spans + ONNX speaker embeddings, fully offline. Advisory — without a speaker model nothing is diarized and layouts/captions are unchanged.
_Avoid_: speaker detection, voice ID

**Speaker turn**:
A span of audio attributed to one voice. Turns split only at silence-gap midpoints, so a cut keyed to a turn boundary never lands mid-sentence.
_Avoid_: utterance, segment

**Split**:
Two-person layout for a wide two-shot: two stacked panels, each a locked crop on one face (left face on top). Chosen when two substantial persistent faces co-exist in the same frames and the clip holds two voices.
_Avoid_: side-by-side, grid

**SpeakerCrop**:
The Locked crop window hard-cutting between faces at Speaker-turn boundaries — for multi-cam sources where each speaker has their own shot. A cut, never a pan.
_Avoid_: auto-switching crop, face-follow

**Composite score**:
The weighted sum of a Candidate's seven validator scores (self-contained ×2, payoff ×1.6, opening strength ×1.4, clarity ×1.2, tension/novelty, specificity, minus context-dependency and slop-risk), plus a 25–60s duration nudge. Surfaced on every Clip card and in the rejected list.
_Avoid_: virality score, AI score

**Scene guard**:
The validation rule that a Clip may not span a detected scene transition: cuts inside ±500 ms of a boundary snap to word boundaries or the Candidate is rejected.
_Avoid_: shot detection

**Cold-open guard**:
The validation rule that a Clip may not open on a greeting, housekeeping line, or lone filler word — every Clip starts mid-thought.
_Avoid_: hook check

**Zoom cuts**:
Per-Clip opt-in `zoompan` punch-ins on energy/emphasis beats inside the Locked crop. Zoom rests at 1.0 outside rise/fall; retimed through Auto-cut removals.
_Avoid_: ken burns, animated crop

**Caption style**:
The per-Clip caption look — Impact (default karaoke), Clean, Pop (per-word pop + keyword accent), Cinema (lowercase letterspaced fade) — applied at caption time from the Base clip, so restyle is seconds not a re-render.
_Avoid_: template, preset

**Export pack**:
The per-Clip share bundle: `.srt`, `.vtt`, and `.meta.json` (title/description/hashtags) beside the rendered MP4.
_Avoid_: publish, distribution

**End card**:
Opt-in 1.2 s "Made with Clipping Factory" tail appended to a Clip's render.
_Avoid_: outro, watermark

**Progress bar**:
Opt-in thin accent-colored fill strip along a Clip's bottom edge.
_Avoid_: scrubber

**Hook title**:
Opt-in ~1.8 s ALL-CAPS title card burned into a Clip's opening frames, from the Clip's headline.
_Avoid_: title card, intro

**Focus prompt**:
Optional free-text direction ("clips about pricing") that steers LLM candidate selection; the heuristic fallback matches keywords instead.
_Avoid_: topic filter

**Sample episode**:
The bundled `assets/sample-episode.mp4` (regenerable via `evals/make_sample_episode.py`) powering zero-input first run via `POST /api/projects/sample`.
_Avoid_: demo, fixture
