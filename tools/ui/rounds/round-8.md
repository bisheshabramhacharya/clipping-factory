# Round 8: the state vocabulary round

Six calls were prepared and five answered: four absolute panels at 1280 and
1440, and one defect hunter. `defect-hunter-1280-b` was stopped without an
answer after running far longer than a defect pass should, and is not counted.

This is the first round where the panels read the shots directory live rather
than a frozen copy, so an image could in principle change while a call was open.
The library's status label did change mid-round (see the frozen-panels note at
the end); every claim below was re-checked against the DOM before it was acted
on.

## Scores

| panel | screens | median |
|---|---|---|
| absolute-1280-a | upload, staged, processing, error, setup | 6.5 |
| absolute-1280-b | studio, rejected, editor | 6 |
| absolute-1280-c | theater, done, library | 6 |
| absolute-1440-a | six screens at 1440 | 7 |
| defect-hunter-1280-a | upload and processing | 10 findings |

Category medians across the four absolute panels: typography 6, hierarchy 6.5,
spacing and rhythm 7, color and contrast 7, component consistency 6,
iconography 6, primary action 7, empty and error states 5. Median of medians:
6.25.

Round 7 stood at 6.0 median of medians on the same rubric, so this is a quarter
of a point, and it is not close to where the work needs to be. Spacing, color
and primary action moved up; typography moved down; empty and error states did
not move at all and remain the floor.

## What the panels cited, by screen

- **Setup banner.** 200px of band pushes the drop card from y=132 to y=277 and
  knocks the settings below the fold; it offers no control of its own, and the
  black choose-file button below stays enabled while the pipeline cannot run.
- **Processing.** The same progress value appears three times, at three
  positions. The failed row carries no gutter mark where the finished rows carry
  a green check, and its only signal is a red word 537px away. The error screen
  names the failure before it names the file.
- **Studio.** SCORE and LENGTH sit 2px apart at 10px and render as one word; the
  clip count is printed twice within 60px in two orders and two tones; the idle
  primary changes its label from a verb to a status; two black controls appear
  at once while a clip is dirty.
- **Rejected.** Expanding the section at 1280 changes nothing in the viewport,
  because the section opens below six rows inside the same scroller.
- **Theater.** The largest text on the screen is the engine's truncated label,
  so the subject of the screen where a person decides is a sentence cut at
  "AI...". The verdicts are three identical buttons with nothing marking a
  preferred reading, and a download link sits directly beneath them.
- **Library.** A failed row offers no recovery and gives no cause beyond its
  status words; metadata wraps so a size unit is orphaned; in the highlighted
  row the two controls do not share a baseline with the other rows.

## What was disproved, again

- "The failed row's status is the same grey as Complete." It is
  `rgb(163, 33, 22)`; a pixel scan of the shot finds red at x=898, y=240-243.
- "The failed row looks identical to a completed row." Its status line is nine
  words long and coloured; the completed rows are one grey word.
- "Two carats in the font select", "two selected rail rows" and "the progress
  bar disagrees with its label" were already disproved in round 7 and did not
  reappear.

## Note on frozen panels

`prepare.mjs` hands absolute panels absolute paths to the shots directory rather
than copies, so a re-shoot during a round can change what a late reader sees.
Round 9 freezes the set before the first call opens.
