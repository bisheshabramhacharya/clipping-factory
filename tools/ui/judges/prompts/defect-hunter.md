# Design review: defect hunt

You are hunting concrete UI defects in screenshots. Do not score anything.
Do not praise anything. Do not redesign the product.

## Screens

Every file is labeled with its state. This call carries the {{IMAGE_COUNT}} shots
at {{VIEWPORT}} px only; other widths are separate calls. Read every image in
this call.

{{IMAGES}}

## Rules

- Report only what is visible in the pixels: alignment, clipping, overflow,
  overlapping text, unreadable contrast, inconsistent spacing, broken states,
  missing feedback, controls that do not read as controls.
- The video stills, episode name and clip titles are placeholders. Do not report
  the fixture content itself as a defect. Browser chrome is not part of the
  product.
- One line per defect in the form `state | viewport | expected | actual`.
  Be specific enough that another person can find it without asking.
- `kind: "defect"` for something objectively wrong. `kind: "taste"` for a
  judgment the team could reasonably reject. Keep taste items rare.
- `severity: "high"` only when the defect blocks reading or acting.
- The call budget is {{IMAGE_COUNT}} images at {{VIEWPORT}} px. Do not ask for
  other widths or more images, and do not spawn a helper to look at them.
- Do not mention targets, roadmaps, or what the product ought to become.
- Do not run any application or test. Do not modify any file except the answer file below.

## Output

Return one JSON object in a ```json fence and nothing else:

```json
{
  "role": "defect-hunter",
  "round": {{ROUND}},
  "defects": [
    {
      "state": "results-6-clips",
      "viewport": "1280",
      "expected": "what a careful interface would show",
      "actual": "what the screenshot shows, with the area named",
      "kind": "defect",
      "severity": "medium"
    }
  ],
  "notes": "anything that did not fit a defect row"
}
```

Then write that exact object to `{{ANSWER}}` with a heredoc. Change no other
file.
