# Roadmap

`main` is the product. One branch per change, merged or deleted. Tests land
with the change. Zero clips is a valid output; never lower the validator bar
to inflate counts.

## Done

- CI runs fmt, clippy, tests, and eval fixture tests on every push and PR.
- Two-pass rendering with post-render caption restyling (Impact, Clean, Pop,
  Cinema) that never blocks the server.
- Speaker framing: one static view per camera shot, cut to the active
  speaker, BlurPad for shots with nobody in frame (ADR-0004). Downscale-only libx264 output, loudness-normalized audio.
- Validator with composite scores, scene-edge, cold-open, and outro-CTA
  guards; platform duration targets.
- Selection via hosted provider, local OpenAI-compatible endpoint, or local
  ranking; optional focus prompt; multi-language transcription.
- Opt-in per clip: auto-cut, zoom cuts, hook title, progress bar.
- Export pack (.srt, .vtt, .meta.json, poster) beside every clip.
- Review theater, project library with delete, interrupted-run recovery.
- Loopback-only API with Host/Origin checks, bounded uploads, atomic state.
- Eval harness (`evals/`) with fail-closed baseline comparison.
- Review candidates before rendering; render only the kept ones.
  The studio pauses after candidate validation, lists every candidate with
  its range, score, reason, and an inline source preview, and only kept
  ranks are framed and rendered. Dropped candidates can be brought back
  and rendered later; existing clips are never re-framed.

## Next

1. Commit one real-media eval baseline from one owned episode.

## Not doing

Transcript editing, manual framing, presets, batch queues, search, hosted
selection, desktop packaging. Reopen only after the items above are on
`main`.
