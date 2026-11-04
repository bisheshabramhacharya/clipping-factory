# Round 1: comparative smoke test (typography)

Not a round. One category, run to check two things: does the blind pair produce a
checkable citation, and does a second call agree with the first. The other seven
categories are `not_judged`.

This record uses the rule in force on the day: one call per category, no order
check. Every round from round 2 on judges both orientations, and a verdict that
flips with position is filed as a tie.

## Panel

A = this studio, `results-6-clips@1280` and `@1440`. B = T3 Code's real window,
1280x800 and 1440x900. Four images, one call. Label map in
`comparative-map.json` in this directory.

## Judge

`deepseek/deepseek-v4.1-flash`, one Capy subagent per call on the shared
machine, reasoning `low`. The pinned setting is `high` in `judges/judges.json`;
`high` did not converge on this panel (13.31 credits, 939k tokens, no answer,
stopped), so the two calls below ran at `low`.

## Call 1: reply-only, 7.76 credits, 581k tokens

Winner: **A** (this studio), confidence `clear`.

> A/02.png: the panel's micro-labels "CAPTIONS" and "EXTRAS" are both set at the
> same ~11px uppercase with ~0.08em tracking, while the six clip titles all sit
> at 14/19 and their durations all at 13px, so the same kind of value gets the
> same treatment in every row; B/02.png runs one muted ~13px weight across the
> breadcrumb, sidebar rows and field labels, so hierarchy there is carried by
> color, not by type.

Why: A has a real scale with tracked uppercase micro-labels, a consistent
heading/body pairing and matching numeric treatment; B is clean but nearly
single-size, leaning on gray levels rather than type.

## Call 2: file write, 17.95 credits, 559k tokens

Winner: **A**, confidence `slight`.

> A/01.png: the section labels 'CAPTIONS' and 'EXTRAS' are set at about 10px,
> uppercase, with roughly 0.08em letter-spacing, and the same treatment repeats
> in A/02.png at the wider width, sitting one step below the 18px/700 page title
> '6 clips' and a second step below the 13px/400 card titles, a three-level
> scale that holds across both windows. B/01.png and B/02.png spend every
> non-heading label in the same plain 12-13px sentence case ('Open in VS Code',
> 'Add project script', 'Initialize Git', 'Ask for changes, send follow-ups, or
> attach images'), with no letter-spaced or caps label level at all.

Why: A carries a recognizable scale of title, body and caps label. Neither panel
is clean on the small end: A's 10px labels and 11px meta lines sit under the 12px
floor, and B's muted sidebar text runs at low contrast.

## Is the citation checkable

Yes. Both calls name the same two elements on each side, and both can be checked
against the images and the CSS:

- `static/styles.css:447` sets `.restyle-label` at 11px, weight 700, letter
  spacing .06em, uppercase. The judges said 10 to 11px and 0.08em, so the level
  is right and the tracking is overstated by two hundredths of an em.
- The B-side labels they quote (`Open in VS Code`, `Add project script`,
  `Initialize Git`) are all visible in `B/01.png`.

Nothing in either citation is invented. One number is slightly off, which is
what a citation is for: a person opens the image and the stylesheet and sees it.

## Is the verdict stable

Within this model, yes: two independent calls both give the win to A, with
different confidence (`clear`, `slight`) and different wording.

Across models, no. The same panel judged by `claude-opus-5-5` gave the win to B
and cited the studio's semibold spread across nav, buttons, tabs and toggle
labels. That answer is kept as `superseded/claude-opus-5-5.json`. Two models
looking at the same pixels read the same facts differently: our caps labels are
a scale to one judge and decoration to the other. That is an argument for
recording the citation as the finding and for not treating a single call as
truth.

## Cost

- Call 1 (reply-only): 7.76 credits, 581k tokens.
- Call 2 (file write): 17.95 credits, 559k tokens.
- Failed `high` attempt: 13.31 credits, 939k tokens, no answer.
- Vision probe on the same model: 7.22 credits, 109k tokens.

Total spent on this backend: 46.24 credits. Per-call token use, not the model's
price, is what makes these calls expensive.
