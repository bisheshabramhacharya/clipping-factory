# Reference panel

Nine images from three other products, kept outside this repository's own UI so
a comparative review has something real to anchors on. `manifest.json` gives
every file a `role`, and the role decides whether a judge may score against it.

## role: reference

Two files, both T3 Code's real window at a native size:

- `t3-code-window@1280.png` (1280x800) and `t3-code-window@1440.png` (1440x900),
  captured locally on 2026-11-03 from the installed app,
  0.1.1-nightly.20261020.3386, with `capture-t3-window.mjs`.

These are the only files a comparative judge is scored against. They are the
same kind of image as `../shots/*.png`: the app's own Chromium renderer at a CSS
viewport size, device scale factor 1, no macOS title bar and no window shadow.
That is what makes a comparison between the two sides mean anything.

## role: aspiration

Seven files of vendor marketing art, 720 to 1400 px wide. They are cropped
single surfaces, not whole app windows at a native size, and they were shot for
a landing page rather than for review:

- **T3 Code** (1 file): the desktop-app image from the product site. Superseded
  by the two captured windows above.
- **Opus Clip** (3 files): clip editor, a finished vertical clip, clip grid.
- **Descript** (3 files): editor, Underlord prompt, Underlord chat edit.

Nothing is scored against them. They stay in the panel because they are useful
to look at: they show what the category leaders put on screen, and a person can
compare a decision against them by eye. A judge that scores against a 720 px
marketing crop is measuring a resized screenshot, not an interface.

`capture-t3-window.mjs` is the capture method for the reference role, kept
next to the files so the same window can be re-shot after an app update:

```bash
open -na "/Applications/T3 Code (Nightly).app" --args --remote-debugging-port=9333
node tools/ui/reference/capture-t3-window.mjs 9333 tools/ui/reference
```

`node` drives the running app's DevTools endpoint, sets the viewport, and writes
both PNGs. Change no other file. These are third-party assets used for internal
comparison only; do not redistribute them as part of the product.
