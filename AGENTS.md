# AGENTS.md

## Health Stack

- typecheck: cargo check --all-targets
- lint: cargo clippy --all-targets -- -D warnings
- fmt: cargo fmt --all --check
- test: cargo test
- shell: bash -n evals/run.sh
