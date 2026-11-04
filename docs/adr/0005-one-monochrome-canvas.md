# One monochrome canvas, hairline structure, one action per screen

The studio shipped with a warm paper ground, white cards floating on it, a
terracotta accent, and pills for every control. Four rounds of correctness work
inside that frame (13 border radii down to 5, 13 font sizes down to 6, three
typefaces down to one) produced a cleaner version of the same look and no real
change in how the product read.

Decision: the interface is one white canvas. Structure comes from 1px rules,
white space and one recessed fill, not from cards. A shadow means exactly one
thing, that something floats above the page, and the only surfaces that carry
one are the library drawer, the dialogs and the toast. One typeface: Inter,
inlined as a base64 `@font-face` because the server has no binary font route.
Eight sizes (10, 11, 12, 13, 15, 20, 26, 34), and 34 is a single display size
used on exactly one element, the upload hero; clip titles stop at 26, which is
the subject of the screens they head. Radii are 4 for chips, 6 for controls, 10
for panels and 999 for pills. Text is three ink levels, all passing 4.5:1 at
their smallest use. Status green, amber and red appear only where something is
ok, waiting or broken. Nothing else is allowed a hue.

Numbers are set as a ledger: labels in a gutter, tabular figures on the value
side, one rule above the group. That is the same construction on the upload
settings, the inspector's In / Out / Length block, the processing step list and
the library's project rows, so four different screens read as one document.

One filled action per screen, and it is the action the screen exists for. In
the studio that action is Review clips, because the run's next step is deciding
which clips to keep; the clip download is outlined beside it. In the clip
editor it is Apply changes, and only while the clip is dirty. In the review
theater nothing is filled until a decision is recorded, then the chosen verdict
fills. This keeps a screen from ever showing two primaries: the earlier build
had both Review clips and Download MP4 solid black in the same viewport, so
neither read as the next step.

The three zones (clip list, 9:16 plate, inspector) survive, because they match
how the work is done. What changed is their construction: the rail is a list
with one row rhythm and no cards, the plate is centred in its column with no
media shadow, and the inspector is a reading column of labelled sections
separated by hairlines rather than a panel of boxes. The rail, the sections and
the reviewer's spread are all one scrollport each, so a panel that overflows
scrolls inside itself and the window never scrolls.

Evidence and the instrument that produced it live in `tools/ui/`:
deterministic shots of eleven states at three viewports, a mechanical audit
(contrast, overflow, distinct radii/sizes/shadows/families, clipped text,
measured DOM boxes), and blind judge panels. The design was chosen from four
independently built versions ranked blind against each other, not from one
person's preference; `rounds/round-6.md` records the rankings and what
disqualified the leader as first shipped, and `rounds/round-7.md` records the
panel that reviewed this build, the defects it cited, and the three claims that
did not survive measurement.

What this does not touch: the render pipeline, clip framing, caption timing, or
anything about how the clips themselves look. `src/` is untouched by the
restyle; every change is in `static/`.
