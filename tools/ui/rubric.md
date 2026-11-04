# UI review rubric

Eight categories. The absolute pass scores them 1–10; the comparative pass uses
the same definitions and returns a winner instead of a score. The prompt for a
judge uses this file verbatim, so the meaning of a category stays stable from
round to round. The anchors describe 2, 4, 6 and 8; interpolate for odd numbers.
A 1 is broken and a score above the top anchor means the work is
indistinguishable from the best production software you know and leaves you
nothing to note.

Score the set of screens as a whole, not one favorite screen. When a claim is
about a specific screen, name it (`state@width`). A category score is not an
average of per-screen scores.

## 1. Typography

- **2** Several unrelated typefaces or sizes; body text below 12 px; line
  length or line height makes paragraphs hard to scan; weight used decoratively.
- **4** One family, but sizes and weights are picked per element with visible
  inconsistency; headings and body compete; numbers and times use different
  treatments for the same kind of value.
- **6** A recognizable type scale with a few stray sizes; body copy is
  comfortable; headings are distinct from body without shouting; numeric data
  aligns.
- **8** Every size, weight and measure has a reason; hierarchy is legible at a
  glance; long labels truncate deliberately; the same kind of value always gets
  the same treatment.

## 2. Hierarchy

- **2** No clear reading order; the eye lands on decoration before content;
  competing headings at the same level.
- **4** A main heading exists but secondary content is equally loud; unrelated
  actions sit at the same weight; the page has no focal point.
- **6** Clear primary region, secondary regions step down in weight; groups are
  separated by space as well as by type.
- **8** The eye can follow one deliberate path through every screen; grouping is
  obvious without borders; nothing important is buried and nothing secondary
  shouts.

## 3. Spacing and rhythm

- **2** Elements touch or nearly touch; gutters change arbitrarily; alignment
  jumps between sections.
- **4** Consistent page padding, but inner gaps vary without reason; labels and
  values drift; inconsistent vertical rhythm between cards.
- **6** A small spacing scale in use; alignment is consistent within a panel;
  dense areas are deliberately denser, not accidental.
- **8** Spacing communicates grouping; the same gap means the same relationship
  everywhere; edges line up across panels; density is comfortable at every
  viewport.

## 4. Color and contrast

- **2** Text unreadable against its background; color used with no meaning;
  failures across normal body copy.
- **4** Mostly readable, but low-contrast secondary text, or accent color used
  for both meaning and decoration; disabled and enabled states look alike.
- **6** Body and secondary text meet a normal reading standard; one accent,
  used consistently; status colors are distinguishable from the accent.
- **8** Contrast holds in every state including disabled and muted text; the
  palette is small and semantic; color is never the only carrier of meaning.

## 5. Component consistency

- **2** Buttons, selects and inputs look like they come from different
  applications; the same action has different shapes in different places.
- **4** Shared primitives exist, but radius, height, padding or shadow drift
  between screens; states (hover, active, disabled) are missing or inconsistent.
- **6** One button family, one input family, one card family; sizes come from a
  small set; states are visibly distinct.
- **8** Every control variant is a deliberate member of one system; the same
  component looks identical wherever it appears; nothing is re-invented for one
  screen.

## 6. Iconography and detail

- **2** Missing, mismatched or stretched icons; decorative marks that carry no
  meaning; obvious alignment and clipping artifacts.
- **4** Icons are present but mix styles, stroke weights or sizes; small details
  (badges, dots, separators) are inconsistent.
- **6** One icon family and weight; icons align with text; small details are
  consistent enough not to distract.
- **8** Icons clarify without decorating; optical alignment is right; every
  badge, dot and separator has a purpose and a consistent treatment.

## 7. Clarity of the primary action

- **2** No obvious next step; several actions compete at full weight; the
  primary button is hidden or mislabeled.
- **4** A primary action exists but is not visually first; labels describe the
  mechanism instead of the outcome; destructive and safe actions look alike.
- **6** The next step is obvious on every screen; the primary action is
  singular; labels are verbs in the user's terms.
- **8** Each screen answers "what now?" without reading; the primary action is
  unmistakable and sits where the eye already is; secondary actions stay
  available without competing.

## 8. Empty and error states

- **2** Blank space with no explanation; errors shown as raw text or not at all;
  no way forward.
- **4** A message exists but is vague; the recovery action is missing or
  detached from the problem; empty states look like a broken screen.
- **6** Empty states explain what to do; errors name the failing step and offer
  a way forward; the layout stays intact.
- **8** Empty, loading and error states are designed as first-class screens;
  messages say what happened and what to do; recovery is one obvious action and
  the rest of the screen stays usable.

## Scoring rules

- Whole numbers 1–10. No half points.
- A category is scored from the evidence in the screenshots, not from what the
  product might do off-screen.
- Missing evidence is not a failure: if a state is absent, score the categories
  from the states that are present and say what was missing in `notes`.
- Do not reward or punish the fixture content (fake episode, clip titles,
  placeholder video frames). Judge the interface around it.
- Browser chrome, scrollbars and the operating system's window frame are not
  part of the product.

## Comparison method

The comparative pass exists to stop an absolute score from flattering the work.
It is run as one call per category, on a blind pair, and it has four rules.

1. **Blind panels.** Every panel is copied into `pool/A/` and `pool/B/` under
   opaque names, in an order shuffled per category. The judge is never told
   which side is this studio, which side is the reference, or which product
   either one is. The label map lives in `rounds/round-<n>/comparative-map.json`,
   outside the judge's input directory.
2. **One winner per category, with a citation.** The judge returns `A`, `B` or
   `tie` and must cite a specific, checkable detail from the winning panel: a
   measured distance, a spacing decision, a type-scale move, a control's exact
   height or weight. The citation names the panel file and the value so a
   person can open the image and check it.
3. **A tie is not a win.** No citation means a tie. A tie is recorded as a tie
   and never counted as a win, and the judge must say what is equal and what
   neither panel gets right.
4. **No target score, ever.** A judge is never told the score anyone wants, the
   round's previous results, or that the work is being optimized. A category is
   judged on the two panels in front of the judge.

The budget is part of the method too: at most ten images in one call, no
subagents or helpers, and one retry when a reply cannot be parsed. A call that
spawns a helper is a failed call, because the helper would be scoring the panel
on the judge's behalf.

## Run policy

These lines are for the people running rounds. They are not instructions to a
judge, and a judge should not change what it looks at because of them.

- **Scope.** A round judges at most two categories, chosen from the lowest
  absolute score. Each category costs two calls (see order check) and a round is
  held under 30 credits.
- **Order check.** Every pair is judged twice, once with the sides swapped. A
  verdict that flips when the panels exchange places is recorded as a tie: the
  judge preferred the position, not the design.
- **Reasoning.** The judge runs at `low`. `high` did not converge on a
  four-image panel (13.31 credits, 939k tokens, no answer). `max` returned a
  checkable citation on a two-image panel but cost 12.45 credits for one call,
  which puts a three-category round at about 75 credits, and two of its
  specifics did not survive checking against the stylesheet. `low` cost 6.65
  credits for a two-image call and its measurements matched within .02em.
  Measurements are in `rounds/reasoning-test.md`.
- **Presentation.** A round file leads with the citation and states the verdict
  after it, so a reader adjudicates the evidence instead of reading a score.
