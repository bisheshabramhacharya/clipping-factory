# Clipping Factory Paid Product Research

**Research date:** July 30, 2026  
**Repository:** `codingwithb/clipping-factory`  
**Integration branch:** `agent/clipping-factory-buildout`  
**Epic:** [#2 Clipping Factory Paid Product Buildout](https://github.com/codingwithb/clipping-factory/issues/2)

## Executive conclusion

Clipping Factory is not an empty prototype. It already has the core of a credible local-first media product: local word-timestamp transcription, local or optional API-assisted candidate selection, deterministic anti-slop validation, continuous-source excerpts, face-aware vertical framing, two-pass rendering, caption restyling, filesystem persistence, restart recovery, and per-clip failure isolation.

The strongest commercial opportunity is **not to generate more clips**. Current clipping products already generate many clips. The recurring market complaint is that users must repair selection, boundaries, captions, framing, audio, and exports until the time advantage disappears.

Clipping Factory should sell a different promise:

> Find strong moments locally, prove why they are valid, let a human correct the important details quickly, and export publishable clips with source provenance.

The first paid loop should therefore be:

1. inspect candidates before rendering;
2. correct transcript errors;
3. repair start/end boundaries;
4. correct framing;
5. render only approved clips;
6. verify technical quality and source provenance.

Batch, presets, active-speaker framing, search, and client review should follow after the correction and reliability foundations are stable.

---

## 1. Current product assessment

### What works today

The repository audit found a coherent Rust application rather than disconnected demos.

- **Local ingestion:** streaming MP4/M4V upload with type and size checks.
- **Media inspection:** FFprobe extracts duration, dimensions, codecs, frame rate, and stream presence.
- **Local transcription:** whisper.cpp produces word-level offsets and probabilities; sentence-like units are rebuilt deterministically.
- **Candidate proposal:** a local heuristic ranker works without a key; OpenAI and Anthropic are optional provider seams.
- **Deterministic validation:** candidate duration, source bounds, transcript alignment, verbatim quote evidence, composite score, and overlap are checked before rendering.
- **Faithful excerpts:** every clip is one continuous source interval.
- **Framing:** one persistent face receives a smoothed, speed-limited horizontal crop; uncertain/no/multiple-face cases fall back to blur-pad.
- **Rendering:** two-pass FFmpeg pipeline produces a reusable uncaptioned base and a captioned final.
- **Caption polish:** per-clip style, color, and font changes can re-burn from the base.
- **Reliability:** stage artifacts persist, completed work is skipped, interrupted rendering records are repaired, cancellation is supported, and one failed clip does not discard successful clips.
- **Transparency:** source timestamps, selection reason, warnings, and rejected candidate reasons appear in the UI.

### What is incomplete

- The pipeline renders every accepted candidate automatically.
- There is no durable pre-render approval state.
- The transcript is immutable and English-only.
- Low-confidence words cannot be inspected or corrected.
- Boundaries cannot be edited.
- Framing cannot be corrected per clip.
- Two-person recordings cannot follow the active speaker.
- Audio is encoded but not normalized or technically checked.
- Output validation is limited to file existence/non-trivial size.
- The browser remembers one project ID; there is no project library, migration framework, or portable bundle.
- There is no queue, watch folder, or CLI batch workflow.
- Brand/export defaults are not reusable domain objects.
- There is no local full-text search.
- There is no client handoff package or durable review-round state.
- The eval harness is a scaffold, not a demonstrated quality baseline.
- CI is stored as documentation rather than active under `.github/workflows/`.

### What is fragile

1. **Real output quality is unmeasured.** Unit tests cannot establish would-post quality.
2. **Persistence has no explicit schema version.** New paid state could make old projects unreadable.
3. **Heuristic ranking is lexical.** It can work, but should not be tuned without real episode baselines.
4. **One-face framing has limited evidence.** One-frame-per-second sampling can miss fast changes.
5. **Multi-face behavior is safe but limited.** It always gives up to blur-pad.
6. **Source/candidate/manifest relationships are implicit.** Revision-aware editing requires stable IDs and dependency tracking.
7. **The UI is one large static application file.** Many parallel feature PRs will conflict unless interfaces and ownership are established.
8. **Render settings are hard-coded.** Fixed H.264/AAC settings are broadly compatible, but there is no export profile or proof report.

### Documentation mismatches or misleading risks

- The README emphasizes passing tests, but the repository does not show real-media quality results.
- The roadmap describes CI guardrails as done while the workflow is not active in `.github/workflows/`.
- The footer says only transcript text is sent to an AI provider, which is accurate for optional provider use, but the core product story should lead with the fully offline local ranker rather than an “AI connection” control.
- Post-render Swipe Review exists as an open draft PR and must not be presented as an already-shipped durable review system.

---

## 2. Target customers

### Primary: serious independent creator

Produces one or more long recordings per week and wants several reliable clips without learning a full NLE. Pays when the product prevents a repeated trip into another editor.

### Primary: podcast editor

Needs exact names, captions, boundaries, framing, audio consistency, and predictable rerenders. Pays for control and speed, not novelty.

### Expansion: small agency or content team

Processes multiple clients and episodes, needs presets, queueing, review handoff, search, project history, and auditability. Has the highest willingness to pay for throughput.

### Differentiated niche: privacy-sensitive organization

Cannot upload raw interviews or unreleased media to a third-party cloud. Values local processing, source proof, portable projects, and no mandatory account.

---

## 3. Core jobs to be done

1. **Find:** identify moments worth reviewing without watching the entire source repeatedly.
2. **Judge:** understand why a candidate is strong or weak before rendering.
3. **Correct:** fix names, captions, cut boundaries, crop, and audio without rebuilding in another tool.
4. **Protect truth:** keep every clip as a faithful continuous excerpt and expose source evidence.
5. **Publish:** produce technically valid, platform-safe, consistently named outputs.
6. **Repeat:** reuse show/client settings and process several recordings reliably.
7. **Return:** reopen old work, search the archive, and revise clips later.
8. **Hand off:** let a client review clips without mandatory cloud storage.

---

## 4. Major pain points and current alternatives

| Pain | Frequency | Current alternative | Commercial effect |
|---|---:|---|---|
| Too many weak candidates render | Every episode | Wait, review finals, delete most | Slower feedback and wasted compute |
| Wrong names/captions | Common | Fix in CapCut/Descript/Premiere | Embarrassing errors; workflow break |
| Awkward opening or ending | Common | Recut in NLE | Automation savings disappear |
| Wrong crop or speaker | Common on interviews | Reframe manually in NLE | Strong clip becomes unusable |
| Inconsistent loudness | Common across sources | Audio editor or manual filters | Unprofessional output |
| Unknown export validity | Every delivery | Upload and discover failure/poor compression | Rework and client delay |
| One project at a time | High for agencies | Manual babysitting | Low throughput |
| Repeated brand setup | Every recurring show | Templates in another editor | Retention loss |
| No project history/search | Grows with use | Browse folders and transcripts | Back catalog remains unused |
| Client approvals in email/spreadsheet | Every agency delivery | Cloud review portal or manual notes | Coordination overhead |

---

## 5. Competitive findings

### Paid feature pattern

Competitor pricing and plan boundaries show that users pay for professional control and throughput:

- **OpusClip:** paid tiers add custom fonts, speech cleanup, multiple aspect ratios, brand templates, text/timeline editing, bulk export, NLE export, team workspace, scheduler, and API.
- **Descript:** paid tiers increase media/AI allowances and add higher-resolution export, brand/workspace capability, and professional transcript/timeline editing.
- **Captions:** paid tiers add customizable captions, denoise, keyframes, traditional editing, and higher-volume AI workflows.
- **Vizard:** paid/business tiers remove watermarks, add 4K, scheduling, storage, team members, sharing, brand kits, and higher-volume workflows.

The conclusion is not “copy every feature.” The useful signal is that **correction, consistency, and throughput** repeatedly define paid boundaries.

### Recurring user complaints

Reviews and creator discussions converge on several problems:

- generated clip selection is hit-or-miss;
- only a few outputs may be ready from a much larger batch;
- subtitles can be wrong or out of sync;
- cut boundaries and transitions need fine-tuning;
- moving-speaker framing follows the wrong person or crops badly;
- rendering can be slow;
- crashes and lag can erase the promised time savings;
- transcript word alignment errors make text-based editing destructive;
- multi-speaker cross-talk remains difficult;
- missing captions can require frustrating reprocessing;
- generated B-roll can be inaccurate to the actual conversation.

### Platform/export findings

- YouTube recommends MP4, H.264, progressive scan, 4:2:0, fast start, source frame rate, AAC/Opus, and 48 kHz audio for standard upload workflows.
- Instagram Reels accepts a range up to 9:16 and documents minimum 30 fps and 720-pixel resolution.
- TikTok upload documentation recommends H.264 in MP4 and its ad documentation exposes placement-dependent safe zones.

Platform specifications change. Clipping Factory should store versioned profile definitions and distinguish official hard requirements from advisory safe areas.

---

## 6. Sources and evidence

Research was performed on July 30, 2026. Recheck pricing and specifications before final packaging.

### Official product/pricing sources

- OpusClip pricing: https://www.opus.pro/pricing
- Descript pricing: https://www.descript.com/pricing
- Captions pricing: https://www.captions.ai/pricing
- Captions plan help: https://help.captions.ai/docs/getting-started/plans
- Vizard pricing: https://vizard.ai/pricing

### Official platform sources

- YouTube recommended encoding: https://support.google.com/youtube/answer/1722171
- YouTube upload workflow: https://support.google.com/youtube/answer/57407
- Instagram Reel size and aspect ratios: https://www.facebook.com/help/1038071743007909
- TikTok content-posting media transfer: https://developers.tiktok.com/doc/content-posting-api-media-transfer-guide
- TikTok auction in-feed specifications/safe zones: https://ads.tiktok.com/help/article/tiktok-auction-in-feed-ads

### User/review evidence

- OpusClip G2 reviews: https://www.g2.com/products/opusclip/reviews
- OpusClip caption editing help: https://help.opus.pro/docs/article/how-to-edit-captions
- OpusClip public feedback: https://opusclip.canny.io/
- Descript community/reviews and podcast editing discussions were used as directional evidence for timing, crashes, cross-talk, and trust problems.

### Research evidence

- Recent subtitle research reports that caption errors reduce evaluation of both speakers and content, supporting caption accuracy as a quality/trust feature rather than cosmetic polish.

---

## 7. Strongest advantages

1. **Local-first by architecture, not marketing.** Video and core transcription/ranking can remain on the machine.
2. **Faithful continuous excerpts.** This is strategically stronger than invented hooks or hidden splices.
3. **Deterministic validator.** The product can explain objective rejection reasons.
4. **Optional AI rather than mandatory AI.** Users can remain offline or use their own provider.
5. **Two-pass rendering.** Provides a real foundation for fast correction and variants.
6. **Filesystem-readable projects.** With versioning, this can become portable and auditable.
7. **Honest zero-output behavior.** Returning no clip is better than lowering the bar.
8. **Rust/FFmpeg reliability potential.** The engine can be fast, testable, and distributable without a web-service stack.

---

## 8. Biggest weaknesses preventing payment

1. No approval before render.
2. No way to fix transcript/caption errors.
3. No way to repair boundaries.
4. No way to correct a crop.
5. No proof package for source faithfulness and technical validity.
6. No durable multi-project workspace or migration guarantee.
7. No audio consistency.
8. No agency throughput.
9. No reusable client/show configuration.
10. No measured real-media quality baseline.

---

## 9. Willingness-to-pay analysis

| Opportunity | Target | Pain | Frequency | WTP impact | Tier | Revenue role | Demo <2 min | Defensible |
|---|---|---|---:|---|---|---|---|---|
| Pre-render candidate review | All serious users | Very high | Every project | Very high | Paid | Conversion + retention | Yes | High with explainability/local state |
| Transcript correction/vocabulary | Editors/agencies | Very high | Every project | Very high | Paid | Conversion + retention | Yes | High local/private |
| Boundary editor/partial rerender | Editors | Very high | Common | Very high | Paid | Conversion + retention | Yes | High with faithful constraints |
| Manual framing/safe areas | Editors/agencies | High | Common | High | Paid | Conversion + retention | Yes | Medium-high |
| Provenance/QC report | Agencies/private teams | High | Every delivery | High | Paid/Premium | Conversion + expansion | Yes | Very high |
| Audio normalization/QC | Editors/agencies | High | Common | High | Paid | Retention | Yes | Medium |
| Batch queue/recovery | Agencies | Very high | Daily | Very high for segment | Premium | Expansion | Yes | High local throughput |
| Presets/brand/naming | Recurring creators/agencies | High | Every project | High | Paid | Retention + expansion | Yes | Medium |
| Versioned library/portability | All returning users | High | Frequent | High | Free/Paid split | Retention | Yes | High local-first |
| Active speaker | Interview editors | High | Source-dependent | High | Premium | Expansion | Yes | Medium if reliable |
| Local search | Large libraries | Medium-high | Grows over time | Medium-high | Paid | Retention + expansion | Yes | High privacy differentiation |
| Client review package | Agencies | High | Every delivery | High for segment | Premium | Expansion | Yes | High offline differentiation |
| Eval/CI gate | All users indirectly | Foundational | Every release | Medium direct | Free/internal | Retention protection | No | Operational moat |

---

## 10. Feature scoring

Weights:

- willingness-to-pay 25%
- pain solved 20%
- final quality 15%
- time saved 10%
- differentiation 10%
- frequency 5%
- demo value 5%
- architectural fit 5%
- reliability/testability 5%

Scores include penalties for cloud dependency, misleading output, maintenance, architectural disruption, weak evidence, and unreliable automation.

| Rank | Issue | Score | Group | Main penalty/qualification |
|---:|---|---:|---|---|
| 1 | [#5 Pre-render candidate review](https://github.com/codingwithb/clipping-factory/issues/5) | 9.4 | Core paid value | Central pipeline/UI change |
| 2 | [#6 Transcript correction](https://github.com/codingwithb/clipping-factory/issues/6) | 9.3 | Core paid value | Timing merge/split honesty |
| 3 | [#7 Boundary editor](https://github.com/codingwithb/clipping-factory/issues/7) | 9.2 | Core paid value | Context-removal risk |
| 4 | [#8 Manual framing](https://github.com/codingwithb/clipping-factory/issues/8) | 8.9 | Core paid value | UI complexity |
| 5 | [#9 Provenance and audit](https://github.com/codingwithb/clipping-factory/issues/9) | 8.7 | Quality and trust | Hashing/report complexity |
| 6 | [#10 Audio loudness/QC](https://github.com/codingwithb/clipping-factory/issues/10) | 8.4 | Quality and trust | Normalization can raise noise |
| 7 | [#12 Batch queue](https://github.com/codingwithb/clipping-factory/issues/12) | 8.3 | Professional workflow | Agency-weighted; central scheduler risk |
| 8 | [#11 Reusable presets](https://github.com/codingwithb/clipping-factory/issues/11) | 8.2 | Professional workflow | Must not become settings clutter |
| 9 | [#3 Quality regression gate](https://github.com/codingwithb/clipping-factory/issues/3) | 8.1 | Foundation | Indirect WTP |
| 10 | [#4 Versioned project library](https://github.com/codingwithb/clipping-factory/issues/4) | 8.0 | Foundation | Broad schema surface |
| 11 | [#13 Active-speaker framing](https://github.com/codingwithb/clipping-factory/issues/13) | 8.0 | Quality and trust | Strong wrong-speaker reliability penalty |
| 12 | [#14 Local search](https://github.com/codingwithb/clipping-factory/issues/14) | 7.6 | Professional workflow | Value appears after library growth |
| 13 | [#15 Client review package](https://github.com/codingwithb/clipping-factory/issues/15) | 7.5 | Professional workflow | Segment-specific/browser file limitations |

The score does not define build order alone. Foundation issues #3 and #4 must land before the highest-scoring paid features.

---

## 11. Recommended product tiers

### Free

- local MP4 ingest and inspection
- local transcription and local ranking
- optional BYO provider
- deterministic validation and rejection reasons
- basic automatic framing
- default captions
- one active project
- cancel/retry/download
- basic recent-project reopening

### Paid

- pre-render candidate review and render-selected
- transcript correction and vocabulary
- boundary editor and partial rerender
- manual framing and safe-area preview
- loudness normalization and technical QC
- reusable presets and naming
- full project library, portability, and local search
- basic provenance/source verification

### Premium

- batch queue, watch folders, and CLI automation
- active-speaker framing
- advanced audit packages and batch reports
- portable client review packages
- higher-throughput/hardware scheduling once quality parity is proven

A paid license should not control or store project data. No account should be required for core editing. Licensing should be separable from the local media architecture.

---

## 12. Approved features

### Foundation

- [#3 Real-media quality regression gate and active CI](https://github.com/codingwithb/clipping-factory/issues/3)
- [#4 Versioned project library, migrations, and portable bundles](https://github.com/codingwithb/clipping-factory/issues/4)

### Core paid value

- [#5 Pre-render candidate review and render-selected](https://github.com/codingwithb/clipping-factory/issues/5)
- [#6 Transcript correction, confidence review, and custom vocabulary](https://github.com/codingwithb/clipping-factory/issues/6)
- [#7 Transcript boundary editor and partial rerender](https://github.com/codingwithb/clipping-factory/issues/7)
- [#8 Manual framing override and platform safe-area preview](https://github.com/codingwithb/clipping-factory/issues/8)

### Quality and trust

- [#9 Clip provenance, source verification, and export audit](https://github.com/codingwithb/clipping-factory/issues/9)
- [#10 Loudness normalization and technical audio QC](https://github.com/codingwithb/clipping-factory/issues/10)
- [#13 Conservative active-speaker framing](https://github.com/codingwithb/clipping-factory/issues/13)

### Professional workflow

- [#11 Reusable creator/brand/naming/export presets](https://github.com/codingwithb/clipping-factory/issues/11)
- [#12 Batch project queue and recovery](https://github.com/codingwithb/clipping-factory/issues/12)
- [#14 Local project/transcript/candidate/clip search](https://github.com/codingwithb/clipping-factory/issues/14)
- [#15 Portable client review packages](https://github.com/codingwithb/clipping-factory/issues/15)

---

## 13. Deferred features

### Local learning from accepted/rejected clips

Potentially valuable, but durable review decisions must exist first. Start with transparent analytics on decisions; do not silently train or adapt ranking.

### Multilingual transcription

Commercially useful, but the product should first prove transcript correction, evaluation, and caption quality in English. Language support should be measured per model/language.

### NLE interchange

A simple tested interchange format may be useful after clip revisions stabilize. Do not promise broad Premiere/Final Cut/Resolve interoperability without end-to-end fixtures.

### Optional hosted selection relay

Could simplify API setup, but it weakens the local-first story, creates accounts/operations, and is not needed to prove paid value. BYO provider and local ranker are enough initially.

### Reversible local denoise

Only approve after a local method survives listening tests and does not damage voices. Loudness/QC is safer and more objective first.

---

## 14. Rejected features and reasons

| Idea | Decision | Reason |
|---|---|---|
| Another post-render Swipe Review | Reject duplicate | Draft PR #1 already covers the experiment |
| Generative B-roll | Reject | Often mismatches transcript; creates misleading/slop risk |
| Invented hooks or rewritten speech | Reject | Violates faithful excerpt promise |
| Internal filler-word deletion | Reject | Produces non-contiguous/destructive edits and can sound unnatural |
| Generic AI chat | Reject | Weak job-to-be-done and UI clutter |
| Virality score/dashboard | Reject | Poorly grounded; does not make clips more publishable |
| Decorative analytics | Reject | No direct action or quality improvement |
| Mandatory accounts/cloud storage | Reject | Removes strongest differentiation |
| Auto-posting/social scheduler in core | Reject near term | Credential, privacy, and platform-maintenance burden |
| Full NLE replacement | Reject | Huge architectural disruption without focused advantage |
| Face identity recognition | Reject | Privacy/biometric burden unnecessary for framing |
| Blockchain provenance | Reject | Complexity without user value; local checksums/reports are sufficient |

---

## 15. Dependency map

```text
#3 Quality gate
 ├─ required before selection/transcript/framing/audio/render behavior changes
 └─ establishes real-media acceptance evidence

#4 Versioned persistence/library
 ├─ #5 Candidate review
 ├─ #6 Transcript revisions
 ├─ #9 Provenance/QC
 ├─ #11 Presets
 ├─ #12 Batch queue
 ├─ #14 Search
 └─ #15 Client review

#5 Candidate review
 ├─ #7 Boundary editor
 ├─ #12 Non-blocking queue review state
 └─ #15 Durable review semantics

#6 Transcript correction
 ├─ #7 Boundary editor
 └─ #14 Search active revisions

#7 Partial rerender/revisions
 ├─ #8 Manual framing
 └─ #10 Audio rerender

#8 Manual framing
 └─ #13 Active speaker

#9 Provenance/QC
 ├─ #10 Audio checks
 ├─ #11 Resolved preset evidence
 ├─ #12 Batch reports
 └─ #15 Client package evidence

#11 Preset snapshots
 └─ #12 Batch enqueue
```

---

## 16. Suggested build order

1. **#3 Quality regression gate and CI**
2. **#4 Versioned persistence/library/migrations**
3. **#5 Candidate review and render-selected**
4. **#6 Transcript correction**
5. **#7 Boundary editor and partial rerender**
6. **#8 Manual framing/safe areas**
7. **#9 Provenance/QC contract**
8. **#10 Audio loudness/QC**
9. **#11 Presets**
10. **#12 Batch queue/recovery**
11. **#13 Active-speaker framing**
12. **#14 Search**
13. **#15 Client review package**

A small product launch can occur after #3–#10 if the correction loop and real-media results are strong. Agency premium features should not delay proof of the individual paid workflow.

---

## 17. Parallelization plan

### Recommended maximum simultaneous builders

**Three.** The application has central shared files (`domain.rs`, `api.rs`, `pipeline.rs`, `store.rs`, `state.rs`, `static/app.js`). More simultaneous builders will create predictable merge and schema conflicts.

### Wave 0: sequential foundation

- Builder A: #3
- Then Builder A or B: #4

Do not parallelize #4 with another schema rewrite.

### Wave 1: after #4

- Builder A: #5 candidate review
- Builder B: #6 transcript correction
- Builder C: #9 provenance/QC skeleton and report contract

Before starting, freeze stable ID, revision reference, and API envelope conventions.

### Wave 2: after #5/#6 contracts

- Builder A: #7 boundary editor
- Builder B: #11 preset resolver/library
- Builder C: continue #9 or begin #14 search indexing against stable transcript revisions

Avoid simultaneous editing of the same frontend regions; split pages/modules before feature work if needed.

### Wave 3: after partial-rerender contract

- Builder A: #8 manual framing
- Builder B: #10 audio loudness/QC
- Builder C: #14 search or #15 package generation

#8 and #10 both touch render orchestration. Freeze a render-job/artifact-invalidation interface first.

### Wave 4: professional scale

- Builder A: #12 batch queue
- Builder B: #15 client review package
- Builder C: #14 search if not complete

#12 should be isolated from another pipeline/state rewrite.

### Wave 5: advanced framing

- One dedicated builder: #13 active speaker
- Other builders may work only on unrelated search/package/docs tasks.

### Overlap/conflict map

| Area | Issues touching it | Parallel warning |
|---|---|---|
| `src/domain.rs` | #4–#15 | Freeze extension conventions; merge schema PRs carefully |
| `src/store.rs` | #4–#15 | #4 first; use separate artifact files where possible |
| `src/api.rs` | Most | Split route modules before broad parallel work |
| `src/pipeline.rs` | #5, #7, #8, #10, #12, #13 | Never run two orchestration rewrites simultaneously |
| `src/render.rs` | #7, #8, #10, #13 | Freeze render job/settings interface |
| `src/frame.rs` | #8, #13 | Sequential only |
| `src/transcribe.rs`/caption word identity | #6, #7, #14 | #6 contract first |
| `static/app.js` | Most UI issues | Modularize pages/components or expect conflicts |

### Recommended merge order into integration branch

`#3 → #4 → #5 → #6 → #7 → #8 → #9 → #10 → #11 → #12 → #14 → #15 → #13`

#9 can merge before #7/#8 if it uses optional revision references, but the final audit schema should be updated after those issues.

---

## 18. Major technical risks

1. **Migration/data loss:** every migration needs backup, idempotency, and future-version read-only behavior.
2. **Truth drift:** transcript edits must never imply audio changed; boundaries must remain continuous.
3. **Wrong-speaker automation:** fallback is preferable to confident error.
4. **Render invalidation bugs:** changed boundaries/framing/audio must rebuild only required artifacts while preserving last good output.
5. **UI sprawl:** advanced controls should appear only when needed.
6. **Concurrency/resource exhaustion:** queue defaults must be conservative.
7. **Platform spec drift:** profiles need versions, dates, sources, and advisory/hard distinction.
8. **Local HTML security:** client/audit packages must escape all untrusted text.
9. **Model/tool variability:** record FFmpeg/Whisper versions in evals and provenance.
10. **False benchmark confidence:** a golden set is a ratchet, not universal proof.

---

## 19. Recommended validation experiments

### Experiment 1: candidate review value

Give 5–10 creators a build that stops before render. Measure:

- accepted candidates per project;
- candidates rejected before render;
- render time/disk avoided;
- confidence in top candidate;
- whether users prefer fewer reviewed clips to automatic finals.

Success signal: users reject a meaningful share before render and report higher trust without feeling slowed down.

### Experiment 2: repair time benchmark

For ten imperfect candidates, compare:

- Clipping Factory correction time;
- correction time in the user’s current editor;
- final would-post rate.

Test transcript names, one boundary, one crop, and one audio issue.

Success signal: median correction is materially faster without reduced output quality.

### Experiment 3: provenance willingness to pay

Show agencies a source-verification/audit mockup and an ordinary download flow. Ask which they would send to clients and whether it changes tool choice.

Success signal: agencies treat proof/report as part of delivery, not as decorative metadata.

### Experiment 4: active-speaker threshold

On a two-person golden set, measure:

- correct speaker time;
- wrong speaker time;
- fallback rate;
- unnecessary switch count;
- manual correction count.

Ship only if wrong-speaker time is low enough that users prefer it to blur-pad plus manual framing.

### Experiment 5: agency throughput

Run a five-project overnight batch on low-, medium-, and high-spec hardware. Measure completion, crashes, recovery, wall time, disk, memory, and operator interventions.

Success signal: queue reduces interventions without causing resource failures or worse outputs.

### Experiment 6: price boundary

Test positioning, not fabricated revenue forecasts:

- Free: local automatic clips
- Paid: correction and professional export
- Premium: batch and client workflow

Ask users to choose based on actual workflow demos. The likely paid anchor should be comparable to professional clipping/editor plans, but final pricing must follow conversion interviews and usage costs rather than competitor copying.

---

## 20. Product readiness definition

Clipping Factory is ready to charge for the core paid tier when:

- a real golden set exists and paid-loop changes do not regress would-post quality;
- older projects migrate safely;
- candidates can be reviewed before rendering;
- transcript names/captions can be corrected locally;
- boundaries and framing can be repaired with targeted rerenders;
- failed corrections preserve the last good output;
- technical QC and source provenance are visible;
- no mandatory account, cloud upload, or external AI is introduced;
- the workflow remains understandable to an individual creator;
- every implementation PR targets `agent/clipping-factory-buildout`, includes tests and documentation, and is reviewed before any later merge to production.

## Exact next implementation action

Assign a builder to [#3 Foundation: real-media quality regression gate and active CI](https://github.com/codingwithb/clipping-factory/issues/3).

Do not begin paid engine changes until the project can measure real-media regressions and enforce basic CI.
