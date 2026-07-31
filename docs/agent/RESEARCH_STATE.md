# Clipping Factory Research State

**Session started:** 2026-07-30 19:21 PDT  
**Integration branch:** `agent/clipping-factory-buildout`  
**Protected branch:** `main` — do not modify or merge

## Current research question

Which local-first controls, reliability systems, and professional workflows would make creators, editors, and agencies pay for Clipping Factory without weakening faithful excerpt generation?

## Repository findings

- The product has a real Rust end-to-end pipeline: MP4 ingestion, local whisper.cpp transcription, local or optional API selection, deterministic validation, face-aware framing, two-pass FFmpeg rendering, caption restyling, filesystem persistence, cancellation, and retry.
- Existing work already covers post-render caption style/color/font changes, selectable fill/background framing, steadier single-face tracking, and a draft post-render keyboard Swipe Review PR.
- The current app automatically renders every accepted candidate. It has no pre-render approval gate, transcript correction, boundary editor, manual crop override, project library, batch queue, export profile validation, or durable professional review state.
- Transcription is currently English-only and has no correction or custom-vocabulary workflow.
- Multi-face sources always fall back to blur-pad; there is no active-speaker framing.
- Rendering is sequential, CPU-oriented, fixed at 1080×1920 H.264/AAC, and has no loudness normalization or technical QC report.
- The eval harness is a scaffold. No real-media baseline results were found.
- CI is documented in `docs/ci-workflow.yml`, not active under `.github/workflows/`.
- No existing issue backlog was found. Open draft PR #1 must not be duplicated.

## Market findings

- Paid competitor tiers repeatedly gate text/timeline editing, caption correction, brand kits, multiple aspect ratios, bulk export, team workflow, and publishing preparation.
- Reviews repeatedly report that bad clip selection, inaccurate cut boundaries, caption timing, moving-speaker framing, slow rendering, crashes, and repair work erase the time saved by automation.
- Transcript-based editing is valuable when accurate, but timing/alignment errors and destructive automatic edits reduce trust.
- Platform upload requirements differ and change, making deterministic export validation and safe-area checks professionally useful.

## Decisions made

- Preserve continuous-source excerpts and deterministic validation as the core trust advantage.
- Do not create another post-render Swipe Review issue; PR #1 already covers it.
- Prioritize correction, preview, approval, partial re-render, batch reliability, and verifiable output over generative novelty.
- Keep cloud accounts, auto-posting, generative B-roll, voice cloning, internal filler deletion, and generic AI chat out of the approved near-term roadmap.

## Features approved for issue drafting

1. Real-media golden-set quality gates and active CI
2. Versioned project persistence, project library, migration, and portability
3. Pre-render candidate review and render-selected workflow
4. Transcript correction with confidence and custom vocabulary
5. Transcript-based boundary editor with conservative auto-snap and partial re-render
6. Manual framing override with safe-area preview
7. Conservative active-speaker framing for two-person sources
8. Audio loudness normalization and technical media QC
9. Batch project queue with hardware-aware scheduling and recovery
10. Reusable creator/brand/export presets
11. Local search across projects, transcripts, and clips
12. Portable client review package with durable decisions
13. Clip provenance, source verification, and export audit report

## Features rejected or deferred

- Duplicate post-render swipe review — already covered by PR #1
- Generative B-roll and invented hooks — misleading-output risk
- Internal filler-word deletion — breaks the continuous-excerpt promise
- Generic AI chat or virality dashboard — weak evidence and UI clutter
- Hosted AI relay, accounts, and cloud project storage — deferred until local paid value is proven
- Auto-posting and social scheduler — integration maintenance and credential risk
- Local preference learning — defer until durable accept/reject data exists
- Full NLE replacement — architectural disruption without a focused advantage

## Issues created

None yet.

## Remaining research gaps

- Validate exact implementation boundaries and dependency order while drafting issues.
- Score all approved candidates using the weighted product rubric.
- Create the epic, child issues, product research document, and final parallel build plan.

## Exact next action

Create the epic and research-backed child issues, then write `docs/product/PAID_PRODUCT_RESEARCH.md` with issue links, scores, tier boundaries, dependencies, and rejected ideas.
