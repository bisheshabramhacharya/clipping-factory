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
