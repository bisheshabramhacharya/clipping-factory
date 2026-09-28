# Contributing

Clipping Factory is deliberately narrow: one podcast in, strong faithful clips out. Contributions should make that loop faster, clearer, or more reliable.

## Before you start

Open an issue before a large change. Small fixes can go straight to a pull request.

Keep each pull request focused on one user-visible change or one refactor. Do not combine both unless the refactor is required for the change.

## Local checks

Run the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
bash -n evals/run.sh evals/verify_clip_quality.sh evals/bless_baseline.sh
python3 -m py_compile evals/report.py evals/replay_opening_gate.py evals/clip_checks.py evals/make_fixture.py
python3 -m unittest discover -s evals/tests
python3 evals/make_fixture.py --out evals/fixtures/synthetic-episode.mp4 --seconds 30 --scene-seconds 10
(cd promo-videos && npm ci --ignore-scripts && npm run typecheck)
```

Changes to selection, validation, framing, or captions also need the [golden-set evaluation](evals/README.md). Include the before-and-after result in the pull request.

## Pull requests

A useful pull request explains:

- What changed and why.
- How the change was verified.
- Any visible behavior or output difference.
- Any tradeoff that remains.

Do not commit podcast sources, rendered clips, transcripts, API keys, or local project state.
