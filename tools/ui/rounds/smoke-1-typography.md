# Round 1 — comparative smoke test

Not a round. One category, one blind pair, run to check that the new method can
produce a loss at all. The other seven categories are `not_judged`.

## Method

- Panel: **Typography**. A = this studio, `results-6-clips@1280` and
  `results-6-clips@1440`. B = T3 Code's real window, 1280x800 and 1440x900,
  captured 2026-11-03 from the installed app.
- Four images in one call, blind, order shuffled for this category. The label
  map is `comparative-map.json` in this directory.
- Judge: Claude Code CLI, model `claude-opus-5-5`, one call, no subagents, 52.8
  seconds, $0.382. Record in `comparative/typography/call.json`.

## Verdict

**Loss.** B wins, confidence `slight`.

Citation, verbatim:

> B/01.png: hierarchy uses size and color, not weight. The heading 'What should
> we work on?' (y about 262) is about 28px at regular weight, the 'No project'
> line under it is about 14px, the composer controls ('Opus 5.5', 'Auto (High) ·
> Extra High', 'Full access' at y about 439) share one 14px regular style, and
> sidebar meta ('MacBook Pro', '10h') steps down to about 12px muted. A/01.png
> sets nav ('Library'), buttons ('Review clips'), segment tabs ('Impact') and
> toggle labels ('Hook title', 'Auto-cut') all at semibold, so weight no longer
> marks importance, and the same score appears as '#1 · 8.7' in the list but as a
> bold 'score 8.7' pill in the detail panel.

Why: B keeps a small, quiet scale (about 28/14/12) where each step has a job and
the same kind of value looks the same. A has a workable scale and readable 13px
body, but semibold is spread across almost every control and the clip score gets
two treatments, so headings and controls compete; B shows less dense content, so
the margin is small.

The citation is checkable: `Hook title` and `Auto-cut` are strings in
`static/app.js`, `#1 · 8.7` and the `score 8.7` pill are both visible in
`A/01.png`, and both panels sit in the panel directory next to this file's map.
Anyone can open them and measure the same sizes.

## Second call

An earlier call, under the same method and an almost identical prompt, returned
the same verdict: B wins, `slight`, citing the `Hook title` and `Auto-cut`
toggle labels as semibold at the weight of headings. It cost $0.306 and is not
filed, because the prompt was reworded after it (the shipped prompt no longer
says "do not mention a target score", only "there is no score to hit"). Two
calls, two cited losses, no free win.

## What this proves and does not

Proves: with real reference art and no labels, the same judge that recorded a
free win in round 0 records a loss, with a specific the next fix round can act
on (stop using semibold for toggle labels and secondary headings).

Does not prove: anything about the other seven categories, or that this studio
sits below T3 Code on the whole. One loss on typography is one category.

## Cost

$0.688 for both calls ($0.306 and $0.382). The filed record carries only the
second call's $0.382. Billed to the person's own Claude Code subscription, zero
Capy credits.
