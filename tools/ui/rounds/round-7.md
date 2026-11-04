# Round 7: the second-pass build, judged blind

Nine judge calls on the white restyle after its second pass: five absolute
panels over the 1280 states, one over 1440, two defect hunters, plus one call
that did not run (see below). The judges are the pinned model in
`judges/judges.json`, one subagent per call, each reading only its own panel and
the rubric. No call was told a target score, and no judge was told it was
reviewing a redesign.

## Method

- Panels are split by state so a call carries at most six images.
- Absolute panels score the eight rubric categories 1 to 10; defect hunters
  cite defects with no scores.
- Every claim below was cross-checked against the DOM or against the shot's
  pixels before it was acted on. Three claims did not survive that check and are
  recorded here as disproved, not as fixes.

## Absolute medians, six panels

| category | median |
|---|---|
| typography | 7 |
| hierarchy | 6.5 |
| spacing and rhythm | 6 |
| color and contrast | 6 |
| component consistency | 7 |
| iconography and detail | 6 |
| primary action | 6 |
| empty and error states | 5 |

Median of medians: 6. The spread is wide by panel: 5 on the library, 6 on the
studio and the theater, 6.5 on upload and on the 1440 studio set, 7 on
processing.

## What the judges cited, and what was done

- The library drawer dims the whole product. Measured: the studio behind the
  drawer samples #909090 at (100,300) and #BBBBBB at (300,20) at library-open@
  1280. A 44 percent scrim is what turned the white canvas grey, and three
  panels read that grey as the design. Scrim removed.
- The drawer has no styled controls: the word Open is the tail of a metadata
  string on one row only, Delete repeats in the same ink as the dates, and the
  one failed project gives no reason and no next step. Row affordances, hover
  behaviour and the failed row's copy were rebuilt.
- In the theater, Download MP4 carries the same border, radius and height as
  Maybe and Skip, so it reads as a fourth verdict; Keep's 2px ink edge at 0 keep
  makes recommendation, focus and recorded answer look identical; the tally is
  the quietest text in the bar. All three changed.
- review-done drops the masthead, so the payoff screen looks like a different
  product. Masthead kept.
- The rail shows two unlabeled right-aligned numerals per row, and the accent
  row paints two identical yellow dots when the selected preset and the custom
  value match. Both labelled or de-duplicated.
- SRT, VTT, Meta and Poster are bare words next to a bordered control and read
  as labels. They became one segmented control.
- The checked caption tab looked different on the upload screen and in the
  restyle panel. Root cause: `button:hover:not(:disabled)` outranks
  `.seg-btn.active` (0,2,1 against 0,2,0), so hovering a selected tab repainted
  it with `--fill`. The active rule now carries its own hover case.
- The failed processing screen drops the page title, so the running and failed
  states do not share a frame. The title stays.
- The rig's own rejected-expanded action scrolled the rail to the section, so
  one judge concluded that expanding REJECTED replaces the clip list. The action
  no longer scrolls; that was a rig defect, not a product one.
- "The caption tabs, the font select and the toggle grid each start at a
  different x" did not reproduce: all three start at 961 at 1280 in both states.

## Three claims that did not survive measurement

- "The FONT select draws two carets." A pixel scan of results-6-clips@1280 at
  row y=537 finds one dark cluster, x=1238 to 1242, and nothing at x=1246.
- "Two rail rows read as selected." In the DOM, row 03 is the only `.active`
  element; rows 1 and 2 compute `transparent`, and the shot's pixels at
  (150,170) and (150,300) are #FFFFFF.
- "The overall bar reads 17 percent while its label says 34." Measured fill
  widths: `.overall-bar` 224.4 of 660 = 34 percent, `.mini-bar` 250.8 of 660 =
  38 percent. Both match their labels.

## Not judged

`absolute-1440-a` was staged and never run, so the 1440 set covers the studio,
rejected, theater and done screens only. `defect-hunter-1280-a` produced no
answer file. Neither is counted as a pass or a failure.
