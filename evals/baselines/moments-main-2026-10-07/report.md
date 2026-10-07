# Moments baseline — main @ 8417e75 (2026-10-07)

Scored with `evals/score_moments.py` against `evals/public/*.moments.json`.

| source | moments | covered | recall | top10 judged | would-post | precision |
|---|---:|---:|---:|---:|---:|---:|
| expedition-63-64-crew | 11 | 8 | 0.727 | 10 | 3 | 0.300 |
| house-members-day | 8 | 0 | 0.000 | 0 | 0 | — |
| iss-science-panel | 10 | 0 | 0.000 | 0 | 0 | — |
| wh-briefing-2026-02-18 | 10 | 8 | 0.800 | 10 | 6 | 0.600 |
| **total** | 39 | 16 | **0.410** | 20 | 9 | **0.450** |

## How this run was built

`select-replay` against each episode's saved transcript, energy profile, and
project record; the run dir at `evals/results/moments-main-2026-10-07/` holds
`sources/<id>/selection.json` plus the judged `rubric.csv` (copied here).

## Caveats

- Transcripts for these runs came from the episodes' YouTube caption tracks
  converted to `transcript.json`, not the bundled whisper path. Two episodes
  (`iss-science-panel`, `house-members-day`) ship unpunctuated auto-captions:
  the ranker emits zero candidates on them, so their recall is 0.000 — a data
  ceiling, not a ranking signal. Re-run with whisper transcripts before
  reading these numbers as ranker quality.
- The rubric is a single reviewer's pass; treat precision as ±1 clip.
- `house-members-day` and `iss-science-panel` moments are scored but the
  accepted lists were empty, so no rubric rows exist for them here.

## Worst miss

`wh-briefing-2026-02-18` 0:07:13–0:08:43 — the energy-policy answer that ends
on the red/blue-state payoff line. Rank 8 covers 44 % of it but stops ~51 s
before the payoff, just under the 50 % recall bar: the clip ends mid-answer
on the moment the key marks as the reason to post.

## Rejected-candidate audit

All 33 recorded post-validation rejections across the four episodes were
audited for "right outcome, wrong reason": every one is a real mid-sentence
ending (the three wh/exp rejections) — no clip was dropped for a reason that
misdescribes it, so there is no rescue PR hiding here.

The misses are instead a ranking-order story. Instrumenting the funnel
shows ~350–570 doomed windows per episode dying at the
self_contained/opening_strength/context_dependency gates (vague or
mid-sentence openings), and each missed moment's best *covering* window —
the one reaching the payoff — lands 22–51 s short of a real close and
scores composite ~37–41 against a ~48.9 top-30 cutoff. Long windows pay
`-0.1/s` past 50 s whether or not the extra seconds buy an ending; the
verified-close length tax follows this finding (see the stacked PR).
