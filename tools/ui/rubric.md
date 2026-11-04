# UI review rubric

Eight categories, scored 1–10. The prompt for a scoring judge uses this file
verbatim, so the meaning of a score stays stable from round to round. The
anchors describe 2, 4, 6 and 8; interpolate for odd numbers. A 1 is broken and
a score above the top anchor means the work is indistinguishable from the best
production software you know and leaves you nothing to note.

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
