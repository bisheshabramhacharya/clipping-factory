# G Stack example: Issue #3 quality regression gate

## Example user prompt

> Implement issue #3, the real-media quality regression gate, on a new branch
> from `agent/clipping-factory-buildout`. Use the full G Stack loop: shape the
> product decision, review strategy and engineering, build it, review the diff,
> test it, and open a draft PR. Do not merge it.

## Office hours

### 1. Demand reality

The feature is not directly purchased, but it protects the paid product from
shipping visibly worse selection, captions, framing, or rendering. The demand
signal is the repository roadmap itself: every later quality-sensitive feature
depends on this gate.

### 2. Status quo

The repository has 62 deterministic Rust tests, a manual eval README, a simple
upload loop, and a CI file parked under `docs/`. It does not have active CI,
versioned source metadata, reproducible run metadata, partial-failure
preservation, deterministic aggregation, or baseline deltas.

### 3. Desperate specificity

A maintainer needs to answer one concrete question before merging a media
pipeline change:

> Did this branch introduce source failures or make human-reviewed output
> measurably worse than the accepted baseline?

The answer must be backed by a report that can be attached to a PR.

### 4. Narrowest wedge

Build a local, privacy-preserving quality ratchet:

1. activate deterministic CI;
2. run a versioned local manifest through the real HTTP product path;
3. preserve per-source evidence even when a later source fails;
4. aggregate operational and human-review metrics;
5. compare against a baseline with explicit thresholds.

Do not add cloud storage, an AI judge, production schema changes, or committed
media.

### 5. Observation plan

The first useful observation is a deliberately degraded or failed run compared
with a known-good synthetic baseline. The gate must identify:

- a new source failure;
- a would-post-rate drop;
- a rubric-average drop;
- the exact source error;
- deterministic report output across repeated generation.

### 6. Future fit

The report schema should accept more metrics later without rewriting existing
runs. The runner must remain usable for transcript correction, boundaries,
framing, audio QC, presets, and batch processing work.

## CEO review

**Mode: selective expansion.**

The premise is valid. The roadmap explicitly makes this the first
implementation dependency. The dangerous expansion would be trying to
automatically score creative quality. Human review remains authoritative.

Scope decisions:

- Keep: active CI, manifest, run metadata, partial results, report, baseline
  comparison, thresholds, deterministic fixtures, documentation.
- Add: privacy rule that absolute source paths never enter committed reports.
- Add: a resumable run mode because long local media runs are interruption
  prone.
- Reject: hosted dashboard, database, LLM judge, committed media, exact
  cross-machine performance gates.
- Defer: generating synthetic MP4 fixtures through FFmpeg in CI. The report and
  state parser can be validated without downloading models or exercising the
  full production pipeline.

## Design review

Skipped. This change has no product UI scope. The Markdown report is a
developer artifact, and its hierarchy is specified by operational summary,
human review, baseline status, and per-source evidence.

## Engineering review

### Architecture

- `evals/run.sh` owns orchestration through the real local HTTP API.
- `evals/report.py` owns deterministic parsing, aggregation, comparison, and
  rendering.
- `metadata.json`, per-source `result.json`, and the copied `view.json` form the
  durable evidence boundary.
- `rubric.csv` remains the human input.
- `.github/workflows/ci.yml` runs Rust checks plus deterministic eval fixtures.

This separates side effects from pure report logic and avoids production code
changes.

### Error paths

- Missing manifest or source file fails clearly.
- A missing source records a failed `result.json` and does not discard prior
  sources.
- Upload failures and three consecutive polling failures become source-level
  errors.
- Per-source timeout is configurable.
- Interrupted terminal sources can be skipped with `--resume`.
- Invalid rubric scores are excluded and listed by row number.
- Baseline data without current human ratings warns rather than inventing
  scores.

### Security and privacy

- Media and result directories remain gitignored.
- Reports store source IDs and file names, not absolute paths.
- Transcript content is not copied into the aggregate report.
- No network service is added beyond the existing localhost studio.
- Untrusted strings are escaped in Markdown table cells.

### Performance

- The runner processes sequentially to match current pipeline behavior.
- Reports stream small JSON/CSV files and are negligible compared with media
  processing.
- Timing is recorded but not enforced across different hardware.

## Developer experience review

Target time to first useful run:

1. copy the example manifest;
2. edit local paths;
3. start the studio;
4. run `bash evals/run.sh`.

The command emits the result directory and report path. Errors name the missing
dependency or failed source. No new package manager or project dependency is
required. Python uses the standard library only.

## Test plan

Automated:

- `bash -n evals/run.sh`
- `python3 -m py_compile evals/report.py`
- `python3 -m unittest discover -s evals/tests -v`
- existing `cargo fmt`, `cargo clippy`, and `cargo test`

Fixture coverage:

- one complete source and one failed source;
- accepted, rejected, and duplicate/overlap rejection counts;
- human rubric averages and would-post rate;
- false-accept tracking;
- baseline failure on new source failure and score regression;
- deterministic repeated JSON and Markdown output;
- invalid score handling.

Manual after merge candidate:

- run a local six-slot manifest;
- interrupt and resume one run;
- compare a normal run against a deliberately degraded configuration;
- confirm no media or absolute path appears in Git.

## Completion definition

- [x] Branch created from `agent/clipping-factory-buildout`
- [x] Plan reviewed for strategy, engineering, and developer experience
- [x] Active CI workflow added
- [x] Versioned manifest example and committed thresholds added
- [x] Resumable partial-failure runner implemented
- [x] Deterministic report and baseline gate implemented
- [x] Synthetic fixture tests implemented
- [x] Eval documentation and PR policy updated
- [ ] GitHub Actions green
- [ ] Draft PR opened
- [ ] Independent diff review completed
