# Round 0 — design review

Platforms: macos · not present: windows.

7 absolute issues, 11 hunted defects, 18 findings after mapping the comparative losses.

## Scores

| Category | Absolute | Comparative | Median | Descript | Opus Clip | T3 Code |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Typography | 6 | 7 | **6.5** | +1 | +1 | +1 |
| Hierarchy | 6 | 7 | **6.5** | +2 | +1 | 0 |
| Spacing and rhythm | 7 | 7 | **7** | +2 | +2 | 0 |
| Color and contrast | 7 | 7 | **7** | +1 | +1 | 0 |
| Component consistency | 6 | 7 | **6.5** | +2 | +1 | 0 |
| Iconography and detail | 5 | 6 | **5.5** | +1 | 0 | 0 |
| Clarity of the primary action | 5 | 7 | **6** | +3 | +1 | +1 |
| Empty and error states | 6 | 8 | **7** | +3 | +3 | +3 |

Reference columns are gaps: the comparative judge's score for this studio minus its score for that reference product. Positive means the reference was scored lower.

## Deterministic checks

- 81 WCAG AA pairs (8 unique)
- Distinct values: 13 border-radius, 13 font-size, 6 shadow, 9 color
- Horizontal overflow: 0 shots
- Clipped text: 0 elements

Unique WCAG AA failures:

- `div.shell:nth-of-type(1) > header > div.header-actions:nth-of-type(2) > button#ai-btn:nth-of-type(2) > span.soon-pill:nth-of-type(2)` — rgb(107, 104, 96) on rgb(230, 227, 219) = 4.34:1 (needs 4.5:1), 30 shots, first empty-upload@1280
- `main#stage > section#processing-state:nth-of-type(2) > div#stages:nth-of-type(2) > div.step:nth-of-type(4) > strong` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 6 shots, first processing-mid-run@1280
- `main#stage > section#processing-state:nth-of-type(2) > div#stages:nth-of-type(2) > div.step:nth-of-type(5) > strong` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 6 shots, first processing-mid-run@1280
- `main#stage > section#processing-state:nth-of-type(2) > div#stages:nth-of-type(2) > div.step:nth-of-type(6) > strong` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 6 shots, first processing-mid-run@1280
- `main#stage > section#processing-state:nth-of-type(2) > div#stages:nth-of-type(2) > div.step:nth-of-type(7) > strong` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 6 shots, first processing-mid-run@1280
- `main#stage > section#processing-state:nth-of-type(2) > div#error-box:nth-of-type(5) > div:nth-of-type(1) > p#error-text` — rgb(107, 104, 96) on rgb(239, 227, 222) = 4.43:1 (needs 4.5:1), 3 shots, first processing-error@1280
- `div.inspector:nth-of-type(2) > div.restyle:nth-of-type(3) > div.restyle-scroll:nth-of-type(1) > div.restyle-section:nth-of-type(1) > span.restyle-label` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 12 shots, first results-6-clips@1280
- `div.inspector:nth-of-type(2) > div.restyle:nth-of-type(3) > div.restyle-scroll:nth-of-type(1) > div.restyle-section:nth-of-type(2) > span.restyle-label` — rgb(154, 150, 141) on rgb(255, 255, 255) = 2.95:1 (needs 4.5:1), 12 shots, first results-6-clips@1280

## Findings

### ai-connection-disabled

- **absolute** @1280: expected A full-page capture of the disabled AI connection state. — actual The file is a 1232x56 strip containing only the top navigation, and the 1440 and 1920 variants are both 1392x56, so the state itself cannot be reviewed.
- **defect-hunter (high)** @1280,1440,1920: expected A full-viewport screenshot of the state at each labelled width, so the body of the screen can be reviewed. — actual All three captures are header-only strips: 1232x56 at 1280 and 1392x56 at both 1440 and 1920, so no page body is visible and the 1440 and 1920 files are indistinguishable from each other.

### clip-detail-restyle

- **absolute** @1920: expected Seven distinct accent choices, or one selected indicator on a single swatch, with even spacing. — actual The row ends with a duplicate of the active color (identical RGB 255,79,79 to the selected swatch) separated by a 20px gap and drawn without a selection ring.
- **absolute** @1280: expected Download disabled or gated while the panel is dirty, so the user cannot take an unapplied render. — actual 'Unsaved changes' is flagged in rust on the footer while 'Download MP4' above remains fully enabled and identical to its clean state.

### library-open

- **absolute** @1280: expected Per-item status that is distinguishable at a glance and a single, deliberate destructive affordance. — actual Complete, Failed and Cancelled are the same gray text buried mid-line, one row truncates its status to 'Transcribe...', and five full-weight red Delete links line the right edge with no hover or icon.
- **defect-hunter (medium)** @1280: expected Every file row in the Library drawer uses the same metadata grammar and shows its state as a complete word. — actual Row 1 ends '1.2 GB · open' (lowercase state) while rows 2-4 use capitalised states (Complete, Failed, Cancelled), and row 5 truncates its state to 'Transcribe...' inside the metadata line with a hard ellipsis instead of showing a full state.
- **defect-hunter (low)** @1280: expected The drawer reads as a layer over the results screen. — actual The drawer's left edge at x 861 is a hard cut with no scrim and no shadow: the video preview behind it stays at full brightness and is sliced in half, so the panel reads as a third column that happens to cover the right-hand controls.

### processing-error

- **defect-hunter (medium)** @1280: expected The failed step carries a status marker in the same left-hand slot the completed steps use for their checkmarks, so the failing row is findable by scanning the left edge. — actual Steps 1 and 2 have green checkmarks at x 337; step 3 'Transcribe' has nothing in that slot and its failure is only the red word 'Failed' at x 905-938, at the far right of the row, so the failed row reads as unremarkable in the list.

### processing-mid-run

- **absolute** @1280: expected One progress expression per level, with pending steps legible and one consistent progress color. — actual Progress appears three times (rust step bar with 38%, black overall bar with 34%, and a footer line repeating 38% with a ring), and the pending steps 4 to 7 are ~#9A968D at roughly 2.9:1 on white.

### results-6-clips

- **absolute** @1920: expected One primary action for the screen, with download and apply clearly subordinate or disabled until relevant. — actual 'Review clips', 'Download MP4' and the 'score 8.7' badge all use the same filled black treatment, plus a gray 'Apply changes' in the same panel, so three elements compete at full weight.
- **defect-hunter (medium)** @1280,1440,1920: expected Six accent swatches, each color appearing once, with the selected one marked. — actual The accent row renders seven swatches; the seventh repeats the selected color exactly (measured (255,221,0) at both swatch 1 and swatch 7 at 1920) and carries no label, so two swatches look identical and the extra one has no visible meaning. Same duplicate appears in clip-detail-restyle, where swatch 7 repeats the selected red (255,79,79).
- **defect-hunter (medium)** @1280,1440: expected The nav's right edge lines up with the page content's right edge, the way both left edges line up at x 22-24. — actual The header's rightmost element (the 'Coming soon' pill) ends at x 1240 while the results content below ends at x 1260 (1280 viewport), and at x 1400 vs x 1415 (1440 viewport), leaving a visible 15-20px ragged right edge between the nav and the panels/buttons.
- **defect-hunter (low)** @1280: expected The clip list gives a sign that it continues, and no row is sliced through its content at the viewport edge. — actual The sixth row is cut mid-thumbnail and mid-title at the bottom of the list container (y 779), with no scrollbar, fade or partial-row cue, so '#6 · 6.2 Where the advice stops' looks clipped rather than scrollable.

### results-rejected-expanded

- **defect-hunter (medium)** @1280: expected A time range and the separators around it stay on one line, so a rejected entry reads as quote, range, score. — actual In the Rejected list the range breaks after the hyphen ('00:02-' then '00:19 · score 2.4', and '1:02:30-' then '1:02:58 · score 3.9'), and in two entries the ' · ' separator is left dangling at the end of a line while the value it introduces starts the next one.

### setup-missing-runtime

- **absolute** @1280: expected A titled setup problem block in normal flow above the form, with a per-item icon or severity, a clear primary recovery action, and no overlap with the card. — actual A borderless white strip with a 3px red left edge floats over the upload card, covering its top 75px and clipping the dropzone edge and upload glyph, and lists five equal-weight sentences with no heading and no button.
- **defect-hunter (high)** @1280: expected The dependency banner sits clear of the upload card so the drop zone and its icon stay whole. — actual The banner (x 320-960, y 64-200) overlaps the top of the card (card top y 118) and its bottom edge cuts through the upload icon at x 640: only the tray and a stub of the arrow are visible, and the drop zone's dashed top border is hidden behind the banner.
- **defect-hunter (medium)** @1440: expected The banner clears the card it describes. — actual The banner bottom (y 200) lands on the card top (card top y 165), so the rounded top edge and the dashed drop zone border are covered in the middle while the card's corners poke out on both sides of the banner.
- **defect-hunter (medium)** @1920: expected A missing-dependency notice has a heading, one line per missing item, and a way to act on it (recheck, or a link to the install command). — actual The banner is five unlabeled sentences at one weight in a single text block with no title, no per-item bullet, no dismiss control and no recheck action, floating 66px above the card with no other connection to it.

## Comparative evidence on this studio

The comparative judge recorded no pairwise loss: it scored This studio at or above every reference in all eight categories. Its category evidence still names weaknesses, quoted verbatim below.

- **Typography** (7): D/01.png and D/07.png: one family with a tracked all-caps eyebrow ('READY TO CLIP', 'CAPTIONS'), a clear title step, and column-aligned metadata ('1920x1080 - h264 / aac - 47:12'); the 12 to 13px muted secondary lines and the clipped 'Mar 8, 2026 - Transcribe... - 296 MB' in D/20.png are the only strains.
- **Hierarchy** (7): D/01.png: the drop card is the single focal mass and the header stays quiet, while D/13.png steps rail, preview, inspector down in weight without a single divider doing the work; D/22.png weakens this by showing two identical black pills, 'Review clips' in the header and 'Apply changes' in the panel.
- **Spacing and rhythm** (7): D/13.png holds the same 8px-scale gaps between caption rows, swatches, toggles and footer across 1280, 1440 and 1920; D/03.png is the cost, where the same fixed-width card floats as a small island in a 1920 canvas with most of the page empty.
- **Color and contrast** (7): D/10.png: warm off-white ground, near-black text and one terracotta reserved for failure, with the error panel tinted rather than shouting; D/20.png shows the weak edge, a faded pink 'Delete' on the in-flight row that is nearly invisible against the white card.
- **Component consistency** (7): D/04.png and D/13.png: one pill family covers segmented controls, buttons, toggles and swatches at a consistent 40px height, and disabled 'Apply changes' is clearly distinct from enabled in D/22.png; the swatch row drifts, because D/22.png shows red twice in one row, once ringed as current and once as a palette entry, with no label separating them.
- **Iconography and detail** (6): D/07.png: the step list uses typographic check marks, a text 'Working...' and a thin ring spinner as three different marks for two states, and the upload glyph in D/01.png is the only drawn icon on an otherwise icon-free screen. D/16.png clips the top clip card mid-thumbnail under the rail heading with no fade.
- **Clarity of the primary action** (7): D/01.png makes 'Choose MP4' unmistakable, D/04.png pairs 'Make clips' with a quieter 'Change video', and D/10.png puts 'Retry stage' beside a specific cause; the error panel stacks the secondary 'Choose another MP4' above the primary, which is the one place the order fights the emphasis.
- **Empty and error states** (8): D/01.png is a real empty state with a drop zone and one action, D/07.png carries per-step status plus an overall percentage and ETA, and D/10.png names the failing step, quotes the cause ('whisper-cli exited with code 134') and offers retry in place, with the layout intact in all three.

## Where this studio scored above a reference

- primary_action: D/04.png puts a single black 'Make clips' next to a quieter 'Change video' and repeats that ranking on every screen; A/01.png has no action and A/02.png hides its only one behind a low-contrast circle, so the mock never shows a next step the reader can name.
- hierarchy: D/01.png gives the screen one focal mass, the drop card, with everything else stepping down in weight; B/01.png loads the frame with a prompt label, an input, a preview box, three chips, a dim filmstrip and a full-height video, so the eye has no first stop and the filmstrip disappears entirely.
- empty_error_states: C/01.png shows its only non-normal state as a muted 'Waiting on 4 subagents' line with a plain-text 'Stop' link buried in body copy; D/10.png gives the same situation a bordered panel, the named failing step in the step list, the exact cause, and a primary 'Retry stage' button, with the rest of the screen still readable.

## Confirmation

A finding raised by two or more roles is treated as real; a single-role finding stays a lead until a second role confirms it.

| State | Absolute | Comparative | Defect hunter | Roles |
| --- | ---: | ---: | ---: | --- |
| ai-connection-disabled | 1 | 0 | 1 | absolute, defect-hunter |
| clip-detail-restyle | 2 | 0 | 0 | absolute |
| library-open | 1 | 0 | 2 | absolute, defect-hunter |
| processing-error | 0 | 0 | 1 | defect-hunter |
| processing-mid-run | 1 | 0 | 0 | absolute |
| results-6-clips | 1 | 0 | 3 | absolute, defect-hunter |
| results-rejected-expanded | 0 | 0 | 1 | defect-hunter |
| setup-missing-runtime | 1 | 0 | 3 | absolute, defect-hunter |

## Judge notes

**Absolute:** I read 24 of the 30 files as full images before the image budget cut me off: both variants of every state at 1280 and 1920, plus results-6-clips@1440 and empty-upload@1440. The unread files are the remaining 1440 variants of staged-file, processing-mid-run, processing-error, results-rejected-expanded, library-open, clip-detail-restyle and setup-missing-runtime, plus results-rejected-expanded@1920 and ai-connection-disabled@1440 and @1920; the 1440 shots I did read sit between the 1280 and 1920 versions I judged, and no layout change appeared at that width. The ai-connection-disabled captures are broken: all three are 56px tall strips of the header, and two of the three are the same 1392px width, so nothing about the disabled state beyond the 'Coming soon' badge can be judged and that category leans on the other nine states. Several raw facts are from pixel measurement rather than eyeballing: card bounds 680x582 at every width, pill fills 34/38/40px, duplicate swatch RGB, contrast ratios. The library metadata strings, the rejected item text and the setup banner paths are fixture content and were not scored as copy quality except where they wrap badly. Browser chrome and scrollbars were excluded.

**Comparative:** A is three marketing compositions rather than app screens, and A/01.png crops its own headline chip at the frame edge, so its scores judge the mock UI inside the shot. B and C show no empty, loading or failed screen, and C supplies a single screen, so those categories are marked neutral at 5 for missing evidence rather than as failures. D is the only source that documents the same layout at 1280, 1440 and 1920; it holds alignment at every width but does not scale up, leaving a small card on a large empty canvas at 1920 (D/03.png, D/09.png, D/15.png). Two D details worth calling out beyond the scores: on D/22.png the header 'Review clips' and the footer 'Apply changes' are the same black pill, so global and local primaries compete, and the accent swatch row repeats red in one row with no label telling a current value from a palette choice. Several D screens also clip the scrolled clip rail mid-thumbnail under the heading (D/16.png, D/18.png). Browser chrome and window frames were ignored throughout.

**Defect hunter:** Fix the ai-connection-disabled capture first: the files are 56px tall header crops, and the 1440 and 1920 variants are the same width (1392), so that state is unreviewable. Everything else was measured in pixels on the 30 provided images. No rows were filed for placeholder video stills, episode or clip titles, or browser chrome. The setup-missing-runtime card sits about 13px higher than in empty-upload, which is what pushes the icon under the banner at 1280; the banner is a fixed 640px wide, 136px tall block at y 64-200 at every width.

