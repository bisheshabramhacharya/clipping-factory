# Contributing

Clipping Factory is deliberately narrow: one podcast in, strong faithful clips out. Contributions should make that loop faster, clearer, or more reliable.

## Before you start

Open an issue before a large change. Small fixes can go straight to a pull request.

Keep each pull request focused on one user-visible change or one refactor. Do not combine both unless the refactor is required for the change.

## Local checks

Run the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Changes to selection, validation, framing, or captions also need the [golden-set evaluation](evals/README.md). Include the before-and-after result in the pull request.

## Pull requests

A useful pull request explains:

- What changed and why.
- How the change was verified.
- Any visible behavior or output difference.
- Any tradeoff that remains.

Do not commit podcast sources, rendered clips, transcripts, API keys, or local project state.
