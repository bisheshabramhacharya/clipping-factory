# AGENTS.md

## Health Stack

- typecheck: cargo check --all-targets
- lint: cargo clippy --all-targets -- -D warnings
- fmt: cargo fmt --all --check
- test: cargo test
- shell: bash -n evals/run.sh

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on this repo via the `gh` CLI; external PRs are not a triage surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical labels used verbatim: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
