# Round 1 — one scale, real contrast, and the round 0 defects

Base: `capy/add-a-deterministic-ui` (round 0, 6127d31). This round touches
`static/` only. The rig, the rubric, the judges and the reference panel are
untouched; `rounds/round-1.md` and the regenerated `checks.json` are the only
files this round added under `tools/ui/`.

## Scores

Not a measurement. The eight categories below are the round 0 panel median
beside four absolute passes of one judge model over two-width samples of this
branch. The four passes disagree with each other by up to two points per
category, so the round 1 column is inside the noise; the deterministic numbers
under it are not. The boundary panel is the number that counts.

| Category | Round 0 median | Round 1 passes | Round 1 median |
| --- | ---: | --- | ---: |
| Typography | 6.5 | 7 · 7 · 6 · 6 | 6.5 |
| Hierarchy | 6.5 | 7 · 6 · 6 · 6 | 6 |
| Spacing and rhythm | 7 | 7 · 6 · 6 · 6 | 6 |
| Color and contrast | 7 | 7 · 7 · 6 · 7 | 7 |
| Component consistency | 6.5 | 7 · 6 · 5 · 6 | 6 |
| Iconography and detail | 5.5 | 6 · 6 · 5 · 5 | 5.5 |
| Clarity of the primary action | 6 | 6 · 5 · 6 · 7 | 6 |
| Empty and error states | 7 | 7 · 6 · 6 · 7 | 6.5 |

The recurring complaints across those passes, and what happened to each:

- `setup-missing-runtime` kept a live upload under a banner of missing tools
  (two passes). Fixed: the run path is inert while a dependency is missing.
- The library's Cancelled and Transcribing chips read as the same grey (one
  pass), and the ink chip that replaced one of them broke the chip family (the
  next pass). Now one tinted family with a leading dot for the in-flight state.
- `FONT` was the only field label not using the section-label treatment.
  Fixed.
- The disabled `Apply changes` read as broken. It now says "Up to date" when
  nothing is dirty.
- The 1920 canvas, the drawer's hard edge over the page, and the inspector's
  empty lower half at 1920 came up repeatedly and are unchanged; see below.

## Deterministic checks

| | Round 0 | Round 1 |
| --- | ---: | ---: |
| WCAG AA failures (unique pairs) | 81 (8) | **0 (0)** |
| Distinct border-radius | 13 | **5** |
| Distinct font-size | 13 | **6** |
| Distinct box-shadow | 6 | **3** |
| Distinct color | 9 | 8 |
| Distinct font-family | 3 | 2 |
| Horizontal overflow (shots) | 0 | 0 |
| Clipped text (elements) | 0 | 0 |
| Text pairs checked | 1203 | 1287 |

The fifth radius is `0px`, which is the absence of a radius. The four real
values are `--r-sm` 8px, `--r-md` 12px, `--r-lg` 16px, `--r-pill` 999px.
Six font sizes render: 11, 12, 13, 14, 16, 19. The scale has seven steps; the
24px step only exists in the review theater, which no shot covers.

Two of the token values were chosen to match what the rig already draws: the
preview poster is a canvas with `border-radius: 16px` and
`box-shadow: 0 20px 60px rgba(0,0,0,.45)` injected from `rig.mjs`. Setting
`--r-lg` to 16px and `--shadow-lg` to that exact shadow keeps the fixture still
pixel-identical to the real `<video>` element it stands in for, and it is why
the report can claim four radii and three shadows instead of five and four.
The alternative — leaving the poster's values outside the scale — would have
made the scale read worse than the product actually is.

## Delta per category

**Typography.** 13 font sizes → 6 rendered, and none of them chosen per element:
seven steps in `:root`. Weights collapsed to three jobs (400 body, 500 controls,
650 headings), so weight carries rank instead of everything being semibold. The
11px tracked uppercase micro-label is kept as a deliberate convention and now
also labels the upload form's control groups, so the same kind of label looks the
same in the form and in the inspector. The screen title is the largest type on
the page at 19px; the wordmark is 16px and section headings are 16px, so nothing
in the chrome outranks the page you are on. Numbers that align (times,
durations, scores) keep tabular figures.

**Hierarchy.** One filled primary per screen. `Review clips` is the only black
pill; `Download MP4` and `Apply changes` are bordered. The `score 8.7` badge was
a black pill that read as a button and is now a quiet chip. The inspector's
apply row sits where its form ends rather than floating at the bottom of the
card, so the dead zone between "Edit text" and the footer is gone; it still
sticks when the form is taller than the card (`clip-detail-restyle@1280`).

The headings now follow the screen instead of the app. The brand was the `h1`
at 19px with `6 clips` as an `h2` at 16px under it, so the document said the app
name outranked the result you were looking at. The brand is a `<span
class="wordmark">` at 16px, and each screen's title is its `h1`: `Create clips`
and `Processing` stay `sr-only`, `6 clips` is visible at 19px, the largest type
on the page. Measured at 1280, 1440 and 1920: wordmark 16px/650, screen title
19px/650, clip headline `h3` 16px/650, micro-label 11px. Same sizes at all three
widths.

**Control affordances in the header.** `Library` is the only way into saved
projects and had gone to transparent border on transparent background, so it
read as a label with a number next to it. It is a bordered 34px pill again,
13px/500, with the count chip inside; hover fills `--surface-2` and focus draws
the 2px accent outline at 2px offset. `AI connection` is disabled, so it keeps
the weaker `--hairline` and `--ink-3`, which leaves it reading as a control that
is off rather than as another label; the `Coming soon` pill explains why. Both
carry the same pill-and-border shape, so the header has two controls, one live
and one off, instead of two pieces of text.

**Spacing and rhythm.** Radii 13 → 4 tokens. Card and control radii now come
from one scale, so the header pills, the cards, the panels and the media frames
stop drifting between 4 and 24px. Rail rows tightened from 101px to 99px, which is enough for the six-clip list
to end inside the rail at 1280 (client height and scroll height both 670px)
instead of being sliced by its edge.

**Color and contrast.** 81 AA failures → 0, and not by crawling to the
threshold: `--ink-3` is 5.38:1 on white (it was 2.95), `--ink-2` is 7.08, and
every tier passes on every surface in the palette — white, paper, both raised
surfaces and the three soft tints. The Coming-soon pill, the pending step names
and the restyle section labels were the visible failures. The palette also
stopped using the accent for progress: both bars are ink, so the terracotta is
reserved for the active stage's text and for state, not decoration.

**Component consistency.** One control family: buttons, segmented controls,
toggle rows, chips and swatches share the pill/card scheme and the 500 weight,
with 650 reserved for the primary and for headings. The accent row no longer
renders a seventh circle that duplicates the selected color; the custom accent
is a dashed, labeled control that reads as "pick any color", not as a palette
entry.

**Iconography and detail.** The step list has one mark per state, in one slot:
`✓` done, `✕` failed, a bar and a percentage for the active step. The failed
row is now findable by scanning the left edge. The library's destructive action
became a drawn trash glyph that is quiet until hover; before, five red "Delete"
words lined the drawer's right edge at full weight.

**Clarity of the primary action.** One primary per screen (above), and the error
panel's order now matches its emphasis: cause first, then one actions row with
the secondary on the left and `Retry stage` last.

**Empty and error states.** The setup notice is a titled block in the stage flow
with one line per problem, a severity dot per item and a `Recheck` button; it
takes the card's 680px column, sits directly above it, and its title adds up to
the list under it. It no longer floats over the card or cuts the upload glyph. The error panel
keeps its retry in place and states the failing stage once. The library's
statuses are readable at a glance: a chip with the whole word, colored by state,
with processing projects showing the present participle instead of a truncated
"Transcribe…".

## Not changed, deliberately

- The `ai-connection-disabled` capture is still a 56px header crop. That is a rig
  decision in `shots.mjs`, not a product one: the disabled control lives in the
  header, and the 1440 and 1920 files are the same width. The state's own AA
  failure is fixed; the crop is for the rig owner to decide on (a full
  `empty-upload` screen with the AI control disabled would be reviewable).
- The 1920 canvas. The studio still sits in a 1440 max-width shell, so at 1920
  the panel is an island. Changing that is a layout change, not a design pass.
- The review theater: tokenized, not redesigned. No shot covers it.
- Product copy: unchanged apart from the setup notice's title and the word
  "Custom" on the accent control.

## Verification

- `npm run shots`: 10 states × 1280/1440/1920, **0 differing files of 30**
  (`--verify`), re-run after the heading and header changes.
- `npm run checks`: 0 WCAG AA failures, 5 radius / 6 font-size / 3 shadow
  values, 0 overflow, 0 clipped text.
- `cargo fmt --all --check`, `cargo check --all-targets`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` (301 passed),
  `bash -n` on the three eval scripts.
- Before/after pairs for every state at every width: `docs/assets/ui/round-1/`.
