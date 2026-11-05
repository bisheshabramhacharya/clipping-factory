# Round 11: the calibration build, and the first measurement of judge noise

Nine scoring panels read the build at `9cfcfeb`. Seven were distinct screen
sets; two more were given the exact same five images as `absolute-1440-b`, on
purpose, to measure how much one panel's verdict moves on identical pixels.
Two defect-hunter calls completed as well, which score nothing by design.

## Scores

| panel | screens | median |
|---|---|---|
| absolute-1280-a | upload, staged, processing, error, setup | 6 |
| absolute-1280-b | studio, rejected, editor | 6 |
| absolute-1280-c | library, theater, done | 7 |
| absolute-1440-a | the upload family at 1440 | 6 |
| absolute-1440-b | studio, library, editor, theater, done at 1440 | 7 |
| absolute-1440-c (same images as b) | the same five | 6 |
| absolute-1440-d (same images as b) | the same five | 7 |
| absolute-1440-e (same images as b) | the same five | 6 |

Category medians over the five distinct sets: typography 6, hierarchy 6,
spacing and rhythm 5, color and contrast 7, component consistency 6,
iconography 5 (six after the upload panels, see below), primary action 6,
empty and error states 6. Median of medians: 6.0.

Round 10 was 5.5, with color at 5 and empty/error at 6. The movement is real
but modest: color and empty/error clearly improved, typography and spacing
moved one step, hierarchy did not move.

The four panels that read the studio set at 1440 (b, c, d, e) are the useful
measurement, because they are four samples of one screen set: medians 7, 6, 7,
6, so 6.5. Their category medians are typography 6, hierarchy 7, spacing 6.5,
color 7, components 6.5, iconography 6, primary 6.5, empty/error 6.5.

## The noise measurement

`absolute-1440-b` and `absolute-1440-c` were handed identical image files, in
identical order, with the same prompt. They returned 7 and 6. Between them,
single categories moved two points (iconography 6 against 5, typography 7
against 6, spacing 6 against 7). `d` and `e` then returned 7 and 6 again.

So the spread on one build is about one point at the panel level and up to two
points inside a category. Any claim that a design pass gained or lost half a
point from a single panel is unsupported. From this round on, a change is
measured on the median of at least three panels per screen set, and the two
checkout-additional panels for a set are scored as samples, not as new rounds.

## What the panels cited

The complaints changed level again. Nothing in this round was about a control
being wrong; every citation was about a system or a state:

- **The video plate has no transport.** Three panels, in three different sets,
  said the plate has no play, scrub or timecode, and the only playback clue is
  an 11px "Space play" hint. This is the last undesigned surface in the app.
- **The rejected section opens onto nothing at 1280.** Two panels called it
  high severity and one measured a 247-pixel difference between collapsed and
  expanded. Measured after the fact: the details block started at y=732 inside
  an 800px window, the list at y=775, and the rail did not scroll.
- **The transcript clamp cut at eight lines with 110px of empty space below
  it.** The clamp was saving room it never used.
- **The setup screen was two widths.** The problem rows ended at x=638 while
  the footer ran to x=1200 and the only action sat 465px from the list it
  retries.
- **The step list skipped a number.** The failed stage was hoisted above stages
  1 and 2, so the rendered order read 3, 1, 2, 4, 5, 6, 7.
- **Two primaries at once** in the editor (a claim that did not survive a DOM
  read: Review clips measures white there while two judges read it as filled).
- **Muted labels read faint** (measured #565656 at 7.4:1; a judge measured
  3.3:1 off rendered pixels at 1x, which is what 11px light type does).
- **The disabled prev arrow was a smudge**: a #F3F3F3 disc with a 1.1:1 edge.

## What was measured before acting

The judge claims that did not survive a DOM read:

- "Review clips and Apply changes are both filled black." Review clips measures
  `rgb(255, 255, 255)` in clip-detail-restyle; there is exactly one fill. Two
  judges read an outlined button with an ink border as filled, which is a
  hierarchy signal even though the fact is wrong, and the button steps down to
  a hairline as a result.
- "Secondary labels are 3.3:1." They are #565656, 7.4:1, at weight 500. The
  perception was acted on (the label layer moves to 12px/600), the number was
  not.
- "The caption style control underlines all four options." The 2px rule is per
  cell; the control is 302px at 1280 with a 76px selected cell.

## What the round produced

The system gates were built during this round, after the panels had been
dispatched, and they found in one run what the panels had been finding for
three rounds: two caps sizes for one eyebrow role, six caps trackings for it,
and five sizes carrying between two and eight line-heights each.

The pass that closed them (`78e4f07`) also fixed the rail scroller, the
transcript clamp, the failure in place, the setup column, the meter's three
states, the arrow contrast, the 26px primary, and wrote `tools/ui/spec.md`.
`npm run checks` now reports zero system gates, zero WCAG AA failures, zero
overflow and zero clipped text.
