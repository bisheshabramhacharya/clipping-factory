# Round 9: the unforgiving prompts

Seven panels were prepared. Three answered: `absolute-1280-b`,
`absolute-1440-a` and `absolute-1440-b`. The other four
(`absolute-1280-a`, `absolute-1280-c`, `defect-hunter-1280-a`,
`defect-hunter-1280-b`) failed on a provider key error before writing an
answer, or were stopped after running far past a normal pass, and none of
them is counted here. They were prepared again as round 10 against the next
build rather than reusing this round's images.

Two rig changes landed with this round and both matter for reading it:

- The judge prompts were hardened. A judge now scores the weaker reading of
  anything ambiguous, names the one detail keeping a category from the next
  anchor, and is told never to round up. Rounds 5 to 8 were read with the
  older prompts, so these numbers are not directly comparable to theirs.
- `prepare.mjs` copies every panel's images into the panel's own directory
  before the call opens, so a re-shoot mid-round can no longer change a call
  while it is being read. The round-8 note about a label changing under a
  judge is no longer possible.

One further rig change came out of this round: the prompts told judges to
write their answer with a shell heredoc, which cut long reports at about
6.5k characters. Both prompts now ask for one write with the file tool.

## Scores

| panel | screens | median |
|---|---|---|
| absolute-1280-b | studio, rejected, editor | 5 |
| absolute-1440-a | upload, staged, processing, error, setup | 6 |
| absolute-1440-b | studio, library, editor, theater, done | 6 |

Category medians across the three answered panels: typography 6, hierarchy 6,
spacing and rhythm 6, color and contrast 6, component consistency 5,
iconography 5, primary action 5, empty and error states 6. Median of medians:
6.

The floor moved. Round 8 scored empty and error states lowest and primary
action highest among the weak set; here primary action and iconography are
the floor at 5, and the citations are about one action competing with
another rather than about an action missing.

## The build these panels read

The branch at `456d13c`, before the round-9 pass. Two consequences:

- The theater's caption excerpt did not render in these images. The fixture
  carried the text but the still-running server process on port 4573 held the
  pre-patch module in memory, so every screenshot showed an empty caption.
  No judge in this round could score the excerpt, and none of the theater
  findings below are about it.
- The swept page had not yet been through the pass that fixed the footer
  rule, the reason columns, the off toggles, the custom swatch and the
  library count.

## What the panels cited

- **One action against another.** In `review-theater` Keep, Maybe and Skip
  are three identical 136x32 outline buttons, so the safe choice and the
  rejecting one look alike. In `clip-detail-restyle` two filled black buttons
  compete at once, the header's Review clips and the inspector's Apply
  changes, with no distinction of scope. In `review-done` the green count
  contradicts the two black ones beside it.
- **One value, several treatments.** A score is 20px bold in the panel head,
  13px in the rail and a 10px caps footnote in the theater. The same file
  facts are printed twice on the run screen in two different orders. The
  library count is a bordered chip while the clip count is bare caps.
- **Truncation as a habit.** All six rail titles clamp at two lines in 179px
  and four of them cut mid-word. The theater's own headline ends at "to the
  AI...".
- **The muted end is under-set.** Six of six judges who mentioned contrast
  mentioned the same thing: the progress dashes, the off toggles, the custom
  swatch and the muted labels sit too close to white to read as state.
- **The library's action column is inconsistent.** Open is a bordered button
  on four rows and grey metadata after a middot on the selected row, while
  Delete is bare text beside it and only two of five rows carry a status mark.

## What was done about it

The pass that followed (`0962008`) bounded the rejected section to its own
156px scroller so clips stay visible, put reason values in two fixed columns,
removed the footer rule from behind live text, gave the off toggles an ink-3
track and the custom swatch the same 18px as its peers, drew every step of the
theater meter at ink-3, gave the best clip's Keep a 2px edge, made the library
count plain text, and gave the selected library row the same Open control as
its peers.

The claims that did not survive measurement: the footer rule was already
gone by the time the pass landed, and the reason values had already moved to
fixed columns; both were re-measured at 0 collisions. The header and the
inspector were measured at the same right edge (1256 at 1280), so the ragged
right edge a judge reported was the eye reading the pill's optical inset, not
a real misalignment.
