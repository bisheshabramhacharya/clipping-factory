# Changelog

All notable changes to Clipping Factory are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-12

First public release. A local-first podcast clipping studio: one podcast in,
every strong, faithful clip out.

### Added

- **Local-first clipping studio** — drop one MP4 and get feed-ready vertical
  clips (1080×1920, H.264/AAC) with word-accurate captions. No account, no cloud
  upload, no required AI model.
- **Model-free local ranking** over the full transcript, with optional
  AI-assisted selection (OpenAI or Anthropic). Only transcript text ever leaves
  your machine; keys are stored with user-only permissions.
- **Deterministic anti-slop quality gate** — every clip must clear
  self-containment, payoff, clarity, and context rules before it is allowed to
  render. Zero clips is a valid result; the studio explains what it rejected and
  why.
- **Word-timed captions** with Impact and Clean styles, smart accent colors, and
  caption fonts. Finished clips can be restyled in seconds thanks to two-pass
  rendering that caches the base render.
- **Face-aware reframing** with steadier face tracking (outlier rejection and a
  pan-speed clamp) and selectable output framing: face-tracked vertical crop or
  centered source over a darkened blur background.
- **Swipe review theater** for fast clip triage before export.
- **Energy-aware selection** — per-second audio energy boosts strong moments in
  the local ranker.
- **macOS hardware encoding** (h264_videotoolbox) with libx264 fallback for
  faster renders.
- **Caption-only processing** for short videos that don't need reframing.
- **Resumable projects** — interrupted pipelines pick up from the last completed
  stage; finished clips survive retries.
- **Eval harness** with a versioned golden-set manifest, deterministic quality
  reports, and baseline regression thresholds.
- **CI quality checks** (format, clippy, full test suite) and a complete
  open-source release: README, LICENSE, CONTRIBUTING, and SECURITY.

### Changed

- Selection keeps every strong, distinct moment instead of stopping at an
  arbitrary quota.
- Sharper AI selection prompt with a viral priority ladder: conflict > surprise >
  story > admission > hook > energy.
- Studio explains model-free local ranking, and results are readable on phones.
- Upload color picker replaced with a centered preset palette.

### Fixed

- Zero-length word timing in legacy transcripts is repaired on load.
- Uploads guard against low disk space before accepting large files.
- Unexpected task failures are persisted and surfaced instead of silently
  dropping work; render and completion failures are reported.
- Clip headlines are representative of the moment; repeated claims and reactions
  rank correctly.
