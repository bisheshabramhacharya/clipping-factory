# Clipping Factory Research State

**Session started:** 2026-07-30 19:21 PDT  
**Integration branch:** `agent/clipping-factory-buildout`  
**Protected branch:** `main` — do not modify or merge

## Current research question

What exact build order turns Clipping Factory’s local, faithful pipeline into a paid professional product without adding cloud dependency or misleading edits?

## Repository findings

- Real Rust end-to-end pipeline exists: MP4 ingestion, local whisper.cpp word timestamps, local/optional API selection, deterministic validation, single-face framing, two-pass FFmpeg rendering, caption restyling, filesystem persistence, cancellation, and retry.
- Existing work already covers caption style/color/font changes, fill/background framing, steadier single-face tracking, and a draft post-render keyboard Swipe Review PR.
- Current commercial gaps: automatic render of every accepted candidate; immutable transcript; fixed boundaries; no manual crop; no project library/migrations; no batch queue; no audio normalization/QC; no durable client review or provenance report.
- Transcription is English-only. Multi-face sources always fall back to blur-pad.
- Eval harness exists but no real-media baseline was found. CI is documented, not active under `.github/workflows/`.
- No prior issue backlog was found. Draft PR #1 must not be duplicated.

## Market findings

- Paid competitors repeatedly gate transcript/timeline editing, caption correction, brand kits, multiple formats, bulk workflows, sharing, and professional export controls.
- Reviews repeatedly report bad selection, awkward cuts, caption timing, wrong framing, slow rendering, crashes, and repair time erasing automation benefits.
- Platform requirements and safe areas differ and change; versioned export/QC profiles are professionally useful.
- The strongest willingness-to-pay message is less repair work and more confidence, not more generated clips.

## Decisions made

- Preserve one continuous source interval, deterministic validation, local processing, no mandatory account, and no mandatory external AI.
- Make candidate approval, correction, boundary control, framing control, verifiable output, and batch reliability the paid roadmap.
- Keep active-speaker automation behind manual override and real-media evaluation.
- Use a maximum of three simultaneous builder instances after foundation work.

## Features approved

1. #3 Real-media quality regression gate and active CI
2. #4 Versioned project library, migrations, and portable bundles
3. #5 Pre-render candidate review and render-selected workflow
4. #6 Transcript correction, confidence review, and custom vocabulary
5. #7 Transcript boundary editor and partial re-render
6. #8 Manual framing override and platform safe-area preview
7. #9 Clip provenance, source verification, and export audit report
8. #10 Loudness normalization and technical audio QC
9. #11 Reusable creator, brand, naming, and export presets
10. #12 Batch project queue with hardware-aware scheduling and recovery
11. #13 Conservative active-speaker framing for two-person podcasts
12. #14 Local search across projects, transcripts, candidates, and clips
13. #15 Portable client review packages with durable decisions

## Features rejected or deferred

- Duplicate post-render swipe review — already covered by PR #1
- Generative B-roll, invented hooks, rewritten speech — misleading-output risk
- Internal filler-word deletion — violates continuous-excerpt promise
- Generic AI chat and virality dashboards — weak evidence and UI clutter
- Hosted AI relay, accounts, cloud project storage — defer until local paid value is proven
- Auto-posting/social scheduler — credential and platform-maintenance burden
- Local preference learning — defer until durable accept/reject data exists
- Full NLE replacement — architectural disruption without focused advantage
- Broad AI denoise — defer until a reversible local method survives real-media evaluation

## Issues created

- Epic: #2 `Clipping Factory Paid Product Buildout`
- Child issues: #3 through #15
- Every issue specifies target user, evidence, commercial value, tier, UX, scope/non-goals, architecture, engine behavior, acceptance criteria, tests, risks, dependencies, parallelization, branch, PR base, demo, and definition of done.
- All issues currently use the existing `enhancement` label because custom label-management capability was not available in the connector.

## Remaining research gaps

- No real golden-set media was available in the repository, so actual would-post baselines remain unmeasured.
- Competitor pricing/features can change and should be rechecked before launch packaging.
- Active-speaker feasibility and browser-only offline review behavior require prototypes and cross-platform tests.

## Exact next action

Write and commit `docs/product/PAID_PRODUCT_RESEARCH.md`, update epic #2 with all child links and merge order, then verify branch/files/issues and report the exact first builder issue: #3.
