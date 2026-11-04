# Inter

The UI ships one typeface: Inter, built from the official Inter 4.1 release
(`Inter-4.1.zip` → `InterVariable.ttf`, SIL Open Font License 1.1, see
`OFL.txt`).

`InterVariable-latin.woff2` is that font with two changes, both reproducible:

1. The optical-size axis is pinned at its default, `opsz=14`, so a size token
   renders the same drawing at every size instead of switching cuts.
2. It is subset to Latin-1 plus general punctuation, the arrow and check glyphs
   the interface uses, and the `kern`, `liga`, `calt` and `tnum` features.

```bash
python3 - <<'PY'
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
f = TTFont("InterVariable.ttf")
instancer.instantiateVariableFont(f, {"opsz": 14.0}, inplace=True)
f.save("InterText.ttf")
PY

UNI="U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2190-2193,U+2212,U+2215,U+2713,U+2717,U+FEFF,U+FFFD"
python3 -m fontTools.subset InterText.ttf --unicodes="$UNI" --flavor=woff2 \
  --layout-features='kern,liga,calt,tnum' --output-file=InterVariable-latin.woff2
```

Against the unmodified Google latin subset this file is metrically identical
for every glyph Inter draws (234 measured strings, max delta 0.02px); it is
40,876 bytes instead of 48,432 and adds `←`, `→`, `✓` and `✗`, which the review
theater and the stage markers use.

`styles.css` carries the bytes as a base64 `@font-face` rather than a URL: the
studio serves a fixed list of UTF-8 text assets (`src/api.rs`, `include_str!`)
and has no route for a binary file, so a request for `/fonts/…` is a 404 in both
the app and the shot rig. Inlining is what makes "no external request" true.
This directory is the source of that copy; nothing is generated at build time.
