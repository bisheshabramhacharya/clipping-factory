# Eval harness — the quality ratchet

Clipping Factory's quality lives in output judgment: did it find moments worth
posting, preserve context, keep captions accurate, and frame the speaker
reliably? Unit tests protect deterministic code. This harness adds the
real-media evidence required before changing selection, validation, framing,
captions, audio, or rendering.

Media, models, local paths, and generated clips remain outside Git.

## 1. Create your local manifest

```bash
cp evals/manifest.example.json evals/manifest.json
```

Edit `evals/manifest.json` so every `path` points to a local source. Relative
paths are resolved from the manifest's directory. The committed example covers
the minimum useful distribution:

| Category | What it stresses |
|---|---|
| Two-person interview, clean audio | Happy path |
| Solo monologue | Ranking without conversational turns |
| Panel, 3+ voices | Crosstalk and framing fallback |
| Noisy / room-tone recording | Confidence and caption accuracy |
| Fast or accented speech | Word timestamps and quote matching |
| Actual target content | The distribution the product must serve |

The manifest is versioned, but your filled `evals/manifest.json` and media
should stay local. The runner snapshots the manifest into each result directory
for local evidence. Generated results are gitignored; reports identify sources
by manifest ID, not by local path.

## 2. Run the full set

Start the studio:

```bash
cargo run --release
```

In another terminal:

```bash
bash evals/run.sh
```

Useful options:

```bash
# Compare against the accepted baseline and fail on configured regressions.
bash evals/run.sh \
  --baseline evals/results/20260730T120000Z \
  --thresholds evals/thresholds.json \
  --enforce

# Resume an interrupted run without reprocessing completed sources; failed sources are retried.
bash evals/run.sh \
  --run-dir evals/results/20260730T130000Z \
  --resume
```

The runner:

1. verifies the studio and manifest;
2. snapshots commit, branch, tool versions, platform, safe environment checks, and manifest hash;
3. uploads each source only to the fixed loopback studio at `127.0.0.1:4571` and polls independently;
4. preserves completed source evidence when another source fails;
5. writes deterministic JSON, CSV, and Markdown reports.

A run has this layout:

```text
evals/results/<UTC-run-id>/
  metadata.json
  environment.json
  tools.json
  manifest.json
  rubric.csv
  report.json
  report.csv
  report.md
  sources/
    <source-id>/
      upload-response.json
      view.json
      result.json
```

`evals/results/` and `evals/sources/` are gitignored.

## 3. Score the clips

Watch every produced clip and complete the copied `rubric.csv`.

Scores are 1–5:

- `hook`: do the first three seconds earn attention?
- `standalone`: is the excerpt understandable without episode context?
- `payoff`: does the excerpt land rather than trail off?
- `caption_accuracy`: are words correct and synchronized?
- `framing`: does the crop hold the right speaker without drift?
- `would_post`: use 5 for yes and 1 for no when possible.

Use `decision_error` only when applicable:

- `false_accept`: the system accepted a clip it should have rejected.
- `false_reject`: a rejected candidate should have survived.

Record the reviewer and concrete notes. Human ratings are evidence, not an
objective truth, so reports retain counts and averages rather than inventing an
AI judge.

Regenerate the report after scoring:

```bash
python3 evals/report.py evals/results/<run-id>
```

Compare to a baseline:

```bash
python3 evals/report.py evals/results/<run-id> \
  --baseline evals/results/<baseline-run-id> \
  --thresholds evals/thresholds.json \
  --enforce
```

The default gate fails when:

- the current run introduces any new source failure;
- would-post rate drops by more than 0.05;
- any average rubric dimension drops by more than 0.25.

The committed `evals/thresholds.json` is the review policy. Change it only in a
reviewed PR, and do not loosen a gate merely to make a branch pass.

## 4. PR policy

Before merging a change to any of these areas, attach a baseline delta or state
why the golden set is not applicable:

- `src/select/`
- `src/validate.rs`
- `src/frame.rs`
- `src/captions.rs`
- `src/transcribe.rs`
- `src/media.rs`
- `src/render.rs`
- `src/pipeline.rs`

A quality-sensitive PR should include:

- baseline and current commit SHAs;
- source completion rate, source failures, and failed clip renders;
- ready/accepted/rejected counts;
- duplicate/overlap rejection rate;
- would-post rate and rubric averages;
- false accepts and false rejects;
- material output examples or reviewer notes;
- an explicit explanation for every regression.

No source media, model, transcript text, absolute local path, or rendered clip
should be committed.

## CI coverage

GitHub Actions runs:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
bash -n evals/run.sh
python3 -m py_compile evals/report.py
python3 -m unittest discover -s evals/tests -v
```

CI uses only deterministic synthetic fixtures. It never downloads Whisper
models or copyrighted media and never pretends to judge creative quality.
