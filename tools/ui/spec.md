# Clipping Factory interface spec

This is what the build does. Where the build disagrees with this file, the build changes.

## Type

One family: Inter, with the system sans as fallback. Eight sizes, each with one line-height.

| Size | Line-height | Role |
| --- | --- | --- |
| 10px | 15.5px | Eyebrow only: uppercase, `.1em` tracking, weight 600, ink-2 or a status colour. Panel heads (CLIP SETTINGS, CLIPS), the score label (BEST CLIP), theater labels (TRANSCRIPT, WHY THIS CLIP), setup kicker, stat labels. |
| 11px | 17.05px | Secondary: meta lines, kbd chips, badges, short status text, dates, counts, secondary hints. Ink-2. |
| 12px | 18.6px | Body and controls: buttons, tabs, inputs, selects, field labels (sentence case, weight 500, ink-2), library meta, rail title (weight 500). |
| 13px | 20.15px | Readable emphasis: clip and project names, ledger values, state causes, pending stage labels (weight 500), reasons, empty copy. |
| 15px | 23.25px | The score number (600, tabular, in a lockup with its 10px label) and the theater transcript. |
| 20px | 24px | Clip title in the theater, section and modal titles, the failure heading, stat numbers. |
| 26px | 28.6px | The inspector's clip headline and the empty-state headline. |
| 34px | 35.7px | The page title, one per screen: Drop a podcast, Making clips, N things are missing, N clips worth posting. |

Weights: 400 for running body copy, 500 for labels and controls, 600 for titles and emphasis. Nothing else.

**Eyebrow recipe.** 10px, 600, uppercase, `.1em` tracking, line-height 15.5px, ink-2 unless the eyebrow carries a status. Nothing else is uppercase. A number that sits beside an eyebrow (the score) resets tracking, case and weight to its own 15px role.

**Field labels** are 12px sentence case, weight 500, ink-2. They are not caps and not 10px.

**Secondary text floor.** Any text a person needs to read is ink-2 (#565656, 7.4:1) or darker. Disabled controls keep an ink-3 label and carry the off state through the fill and edge instead.

## Colour

Ink ramp: `#0A0A0A` ink, `#565656` ink-2, `#6E6E6E` ink-3 (marks and disabled labels only). Surfaces: `#FFFFFF`, `#F6F6F6` well, `#F3F3F3` fill, `#E9E9E9` fill-2. Lines: `#E5E5E5`, `#D4D4D4`. Status: `#17693F` ok, `#8A5A00` warn, `#A32116` bad, each with a soft fill. Seven colour values render in text; no hue is added.

## Shape

Radii: 0 for structure (rules, rows, tabs, full-bleed blocks), 4 for chips and kbd, 6 for controls, 10 for the drop panel, 999 for circles. Five values render.

One shadow, on floating surfaces only: `0 20px 48px rgba(0,0,0,.14), 0 1px 2px rgba(0,0,0,.06)`.

Control heights: 32 for buttons, inputs, selects and arrows; 28 for the library's Open/Retry; 24 for icon buttons; 18 for swatches, pills and kbd. Settings rows are 45px (32 control, 6+6 padding, 1 border), so every row in a block shares one pitch.

## Spacing

Gaps come from 4, 8, 12, 16, 24, 32. Inside a block use 12; between blocks use 24 or 32; row padding is 6 (settings), 8 (rail), 12 (stage and library). Section rules close every block, including the last stage row.

## Structure

The content column is 1120px on upload, setup and run screens; the chrome's inline padding matches it, so one vertical line hits the wordmark and the content. The studio (results) is full-bleed: 300px rail, then the stage, then the 420px inspector.

One filled control per view: Review clips when the studio is clean, Apply changes when a draft is unsaved, Keep for the best clip in the theater, Check again on setup, Retry stage on a failure, Download kept clips on completion.

## Marks

One grid: 14px box, 24-unit viewBox, 1.6 stroke, round caps and joins. Glyphs: chevron right, chevron down (selects and disclosures), check, cross, dash, dot, download, folder. State kinds are complete (check), working (dot), failed (cross), cancelled (dash); a row that is merely present carries no mark, and the library gives every row a mark so its gutter never moves.

## Named pieces

- **Rail:** the only scroller in the studio. Its rows keep the selected clip in place when Rejected expands; the rejected list has its own 156px scroll and the rail reports scrollHeight > clientHeight when open.
- **Failure:** the failed stage stays in the list at its number, marked failed, with its cause, remedy, Retry and Start over inside the row.
- **Theater:** transcript clamps at 14 lines (336px); if the payload is longer, Show full transcript expands the copy column into its own scroll. Verdict row: Keep is filled for the best clip until a verdict is recorded, then the recorded verdict takes the fill.
- **Completion:** 720px column; kept rows are links to their clip, wrap at 13px, and all three counts are ink. Green appears only in the REVIEW COMPLETE eyebrow.
- **Library:** 451px drawer, 20px status gutter, every title starts on one edge, delete is a 24px ink-3 icon button with an aria-label, scrim 16% black.
