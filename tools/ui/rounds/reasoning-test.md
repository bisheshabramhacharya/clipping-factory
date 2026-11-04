# Reasoning test: max against low

Purpose: pick the reasoning setting for the judge, and measure what a call costs
so a round can be held under 30 credits. One category, typography, on a
two-image panel (each side at 1280 only), 300-token reply cap.

## max

`deepseek/deepseek-v4.1-flash`, reasoning `max`, one subagent, two images.

- Winner: B, which is this studio on this panel. Confidence `clear`.
- Cost: **12.45 credits**, 185k tokens, about 120 tokens of output in one reply.
- Citation:

> B/01.png: all six clip rows set the score bold at the same left edge (~100px)
> with the rank number muted at one size, and the duration in a mono face
> right-aligned to a single edge (~221px); the section labels CAPTIONS and EXTRAS
> share one uppercase letter-spaced treatment.

Checking it against `static/styles.css`:

- `.rail-dur` is 10px, weight 600, positioned inside each 54px thumbnail at
  `right: 3px`, with `font-variant-numeric: tabular-nums`. There is no mono
  face, and there is no single 221px right edge. **Wrong.**
- `.rail-rank` is one 11.5px, weight 600 line holding rank and score together.
  There is no bold score next to a muted rank. **Wrong.**
- `CAPTIONS` and `EXTRAS` are uppercase, letter spaced labels at 11px, weight
  700. **Right.**

So `max` produced a citation that is checkable and two of its specifics do not
survive the check. It reads like measurement and is partly invention.

## low, on the same pair with the sides swapped

`deepseek/deepseek-v4.1-flash`, reasoning `low`, one subagent, two images.

- Winner: A, which is also this studio, on the swapped panel. Confidence `clear`,
  so the preference survived the swap.
- Cost: **6.65 credits**, 421k tokens.
- Citation:

> A/01.png: the meta line '#1 · 8.7' and the 'CAPTIONS' eyebrow are both set at
> ~11px caps, while clip titles and the caption tabs sit at ~13px and '6 clips'
> at ~18px, so a four-step 11/13/18/30 scale repeats across three columns and the
> times (00:37, 12:04, 12:41) share one tabular treatment; B/01.png runs its ~30px
> 'What should we work on?' straight into a 13px bold 'No project' subtitle and
> then 13px helper text, collapsing the middle steps.

Checking it: `.restyle-label` is 11px, weight 700, .06em, uppercase (judge said
~11px). `.rail-dur` carries tabular numerals (judge said tabular treatment).
Both hold. The sizes it quotes are within a pixel or two of the stylesheet.

## Decision

Pin `low`. Two reasons, both measured:

1. Cost. `max` costs 12.45 credits for a two-image call. A round is two
   categories with two calls each, four calls, so `max` puts a round at about 50
   credits and a three-category round at about 75, against a 30 credit target.
   `low` at 6.65 puts a two-category round at about 27.
2. Accuracy. `max`'s citation contained two specifics that do not exist in the
   stylesheet. `low`'s numbers matched. A citation that sounds precise and is
   wrong is worse than a vaguer one, because a reader stops checking.

The 300-token output cap held on both calls: each reply was a single JSON object
of about 110 to 130 tokens.

## Cost table, per call

| Setting | Panel | Reply path | Credits | Tokens |
| --- | --- | --- | ---: | ---: |
| max | 2 images | reply | 12.45 | 185k |
| low | 2 images | reply | 6.65 | 422k |
| low | 4 images | reply | 7.76 | 581k |
| low | 4 images | file write | 17.95 | 559k |
| high | 4 images | file write | 13.31 | 939k, no answer |

Full-round estimate at the pinned setting: 2 categories x 2 calls x 6.65 = about
27 credits. At three categories it is about 40. At `max` it is about 50 for two
categories and about 75 for three.

## Caveat on the order check

The two calls above used different reasoning settings, so they demonstrate the
swap mechanism rather than a clean position test. A real round judges both
orientations at the same setting.
