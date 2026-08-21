# Roadmap

`main` is the product. One branch per change, merged or deleted. Tests land
with the change. Zero clips is a valid output; never lower the validator bar
to inflate counts.

## Done

- CI runs fmt, clippy, tests, and eval fixture tests on every push and PR.
- Two-pass rendering with post-render caption restyling.
- Face tracking with outlier rejection and pan-speed clamp.
- Loopback-only API with Host/Origin checks, bounded uploads, atomic state.
- Eval harness (`evals/`) with fail-closed baseline comparison.
- Project library: list and delete past projects.

## Next

1. Commit one real-media eval baseline from one owned episode (#3).
2. Review candidates before rendering; render only the kept ones (#5).
3. Caption restyle must not block the server (#39).
4. Ctrl+C must not leave a project stuck in `rendering` (#40).

## Not doing

Transcript editing, manual framing, presets, batch queues, search, hosted
selection, desktop packaging. Reopen only after the four items above are on
`main`.
