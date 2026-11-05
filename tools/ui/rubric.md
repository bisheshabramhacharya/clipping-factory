# Panel rubric

One panel is a set of products, shown as screenshots of the same screens, in a shuffled order under opaque codes. A judge scores every product in the panel on the same eight categories. Nothing in this file tells a judge which product is which, what the work is for, or what anyone hopes it will score.

## The panel

- Six products, or fewer when a product has no screen for a state. Identical opaque codes across every state, shuffled once so the codes carry no order.
- You are never told which product is the one under construction, which are competitors, or who made any of them.
- Score what you can see. If a product has no screen for a state, score it from the states it has and say so in `notes`.

## How to score

- Eight categories, each 1 to 10. Whole and half points are both allowed.
- The anchors below are 2, 4, 6, 8 and 10. Interpolate for anything between.
- **10** means: as good as, or better than, the best product in this panel for that category. Nothing you would change.
- Before you score a category, name the strongest product in the panel for that category. That product takes the 10. Everyone else is scored by its distance below the strongest: 9.5 is effectively tied with it, 8.5 is a visible gap, 7 or below is a different league.
- If you believe no product in the panel deserves a 10 in a category, you have not named the strongest one yet. Go back and name it. A panel in which nothing can be best is a panel you have not finished reading.
- **9.5** means: best in the panel, with one small thing you would change.
- **8** means professional and deliberate, clearly not the best in the panel.
- A category is scored on the product's whole screen set, not on one favorite screen and not as an average of per-screen scores.

## Issues

Issues are recorded separately from the scores, in `issues`, at most three per product, ranked by what they cost that product. Every product gets its own three, including the strongest: a crowded list is not a reason to leave one unnamed.

- An issue is one concrete, checkable observation: the product code, the state, and what is wrong where.
- A high score does not require an empty issue list. The issues explain what the next version would fix; they do not lower a score on their own.
- An issue lowers a category only when it changes how that category reads at a glance.
- No issue list is complete. Do not pad it to look thorough, and do not withhold one because the score is high.

## 1. Typography

- **2** Several unrelated faces or sizes; body text under 12 px; paragraphs that cannot be scanned.
- **4** One family, but sizes and weights picked per element; headings and body compete; the same kind of value gets different treatments.
- **6** A recognizable scale with a few stray sizes; body comfortable; headings distinct without shouting; numbers align.
- **8** Every size and weight has a reason; hierarchy legible at a glance; long labels truncate deliberately.
- **10** The type is the design, not a container for it. Nothing to change.

## 2. Hierarchy

- **2** No reading order; decoration lands before content; competing headings.
- **4** A heading exists but everything below is equally loud; unrelated actions share one weight; no focal point.
- **6** A clear primary region, secondary regions step down; groups separated by space as well as type.
- **8** One deliberate path through every screen; grouping obvious without borders; nothing important buried.
- **10** The eye lands where the work is on every screen, and a screenshot alone tells you what to do next.

## 3. Spacing and rhythm

- **2** Elements touch or nearly touch; gutters change arbitrarily; alignment jumps between sections.
- **4** Consistent page padding, but inner gaps vary without reason; labels and values drift; vertical rhythm wanders.
- **6** A small spacing scale in use; alignment consistent within a panel; dense areas deliberately denser.
- **8** Spacing communicates grouping; the same gap means the same relationship everywhere; edges line up across panels.
- **10** The grid is felt, not seen. Density is right at both widths and nothing needs a border to explain it.

## 4. Color and contrast

- **2** Text unreadable against its background; color carries no meaning.
- **4** Mostly readable, but low-contrast secondary text, or an accent used for both meaning and decoration; disabled and enabled look alike.
- **6** Body and secondary text clear a normal reading standard; one accent used consistently; status colors distinguishable.
- **8** Contrast holds in every state including disabled and muted; the palette is small and semantic; color is never the only carrier of meaning.
- **10** The palette sets mood, marks state and guides action at once, and every pairing clears the standard with room to spare.

## 5. Component consistency

- **2** Buttons, inputs and cards look like they come from different applications; the same action has different shapes in different places.
- **4** Shared primitives exist, but radius, height, padding or shadow drift; states are missing or inconsistent.
- **6** One button family, one input family, one card family; sizes from a small set; states visibly distinct.
- **8** Every control variant is a deliberate member of one system; the same component looks identical wherever it appears.
- **10** The system is visibly a system: its rules could be written down from three screens, and it never breaks them.

## 6. Iconography and detail

- **2** Missing, mismatched or stretched icons; marks that carry no meaning; visible clipping or alignment artifacts.
- **4** Icons present but mixing styles, stroke weights or sizes; badges, dots and separators inconsistent.
- **6** One icon family and weight; icons align with the text they sit beside; small details consistent enough not to distract.
- **8** Icons clarify without decorating; optical alignment right; every badge, dot and separator has a job.
- **10** The icon set is a designed thing rather than a download: consistent at both sizes, used exactly where a word would be worse, and it gives the product a face.

## 7. Clarity of the primary action

- **2** No obvious next step; several actions compete at full weight; the primary control is hidden or mislabeled.
- **4** A primary action exists but is not visually first; labels describe the mechanism instead of the outcome; destructive and safe actions look alike.
- **6** The next step is obvious on every screen; the primary action is singular; labels are verbs in the user's terms.
- **8** Each screen answers "what now?" without reading; the primary action is unmistakable and where the eye already is; secondary actions stay available without competing.
- **10** The screen seems to have only one action, and it is the right one. Nothing is hidden that should be offered.

## 8. Empty and error states

- **2** Blank space with no explanation; errors as raw text or not at all; no way forward.
- **4** A message exists but is vague; recovery missing or detached from the problem; empty states look like a broken screen.
- **6** Empty states explain what to do; errors name the failing step and offer a way forward; the layout holds.
- **8** Empty, loading and error are designed as first-class screens; messages say what happened and what to do; recovery is one obvious action.
- **10** The failure screen is as good as the best screen in the product: it says what happened, what it cost, and the single move that fixes it.

## Rules that are not negotiable

- Judge the interface, not the content. Fake episode titles, placeholder frames and fixture names are not defects and are not credit.
- Browser chrome, scrollbars and the operating system's window frame are not the product.
- Every claim names a product code and a state (`P3 results`) so it can be checked against the image.
- Never infer what a product does off-screen. Score what is shown.
