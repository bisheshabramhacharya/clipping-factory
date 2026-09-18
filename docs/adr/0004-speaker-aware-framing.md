# Speaker-aware framing for two-person interviews

Single-speaker framing (ADR-0001) locks on the dominant face and never moves —
right for a monologue, wrong for an interview: in a wide two-shot the guest is
cropped out entirely, and in multi-cam sources the crop sits on the listener
while the other host talks. Both are the largest visible gap vs. Opus/Klap for
interview content (Spec #54).

Decision: when a project-level **Diarization** (transcript-derived speech spans
→ ONNX speaker embeddings → clustered turns; `speakers.json`, fully offline)
hears two voices in a clip AND two substantial persistent face clusters exist,
the layout upgrades:

- **Wide two-shot** (both faces detected in the same frames most of the time)
  → `Split`: two stacked panels, each a locked crop on one face. No camera
  motion; each panel is a fixed column.
- **Multi-cam** (faces appear in alternating frames) → `SpeakerCrop`: the
  Locked crop window **hard-cuts** between faces at speaker-turn boundaries.
  Turns only ever split at silence-gap midpoints, so a cut can never land
  mid-sentence. This is a cut, not a pan — ADR-0001's "the camera never moves"
  stands; switching whose face is framed at a conversational handoff is an
  edit, not camera motion.

Speaker→face correlation runs a dense (~4 fps) second detection pass only for
multi-cam two-face candidates and scores each face's mouth-region pixel motion
inside each speaker's turns. A speaker claims a face when ≥55% of their
turn-time mouth motion lands on it; ambiguous correlations fall back to the
dominant face (existing behavior).

Without a speaker model (`CF_SPEAKER_MODEL` / `models/speaker-embedding.onnx`,
a 16 kHz-mono-waveform → embedding ONNX such as a SpeechBrain ECAPA-TDNN
export), nothing changes: no diarization, no two-face layouts, no caption
labels. Single-face shots are unchanged either way. Speaker turns are also
retimed onto the auto-cut output timeline and emitted as "S1:"/"S2:" tags in
captions and a `<clip>.srt` sidecar.
