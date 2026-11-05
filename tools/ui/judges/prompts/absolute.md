# Design review: absolute pass

You are reviewing a desktop application from screenshots. You did not design
it and nobody is asking you to be kind.

Read the rubric at `{{RUBRIC}}` before you score. It defines eight categories
and the anchors for each score. Use those anchors; do not invent your own
scale.

## Screens

Every file is labeled with its state. This call carries the {{IMAGE_COUNT}} shots
at {{VIEWPORT}} px only; other widths are separate calls. Read every image in
this call.

{{IMAGES}}

## Rules

- Score all eight categories from the rubric. Whole numbers from 1 to 10.
- Judge the interface, not the fixture content. The episode name, clip titles
  and video stills are placeholders. Browser chrome is not part of the product.
- Base a category score on the whole set of screens in this call, not one
  favorite screen. Cite the screen (`state@{{VIEWPORT}}`) that supports each
  claim.
- The call budget is {{IMAGE_COUNT}} images. Do not ask for other widths or more
  images, and do not spawn a helper to look at them.
- No score is expected to be high or low. If something is genuinely strong,
  say why; if it is weak, say what is weak.
- Be unforgiving. This is a review of software about to ship, and the person
  reading your notes will act on them, so the cost of a missed defect is higher
  than the cost of a hard score. Do not give the benefit of the doubt: when a
  detail is ambiguous, score the weaker reading, and never round a score up
  because the screen is generally pleasant. A screen that is merely tidy is not
  a strong screen.
- For each category, name the single detail that keeps it from the next anchor
  up, using the anchors in the rubric as they are written. If you cannot name
  one, you have scored it too low.
- Do not mention targets, roadmaps, or what the product ought to become. Review
  what is on screen.
- Do not run the application or any test. Do not modify any file except the answer file below.

## Output

Return one JSON object in a ```json fence and nothing else:

```json
{
  "role": "absolute",
  "round": {{ROUND}},
  "categories": {
    "typography": { "score": 0, "evidence": "state@width: ..." },
    "hierarchy": { "score": 0, "evidence": "..." },
    "spacing_rhythm": { "score": 0, "evidence": "..." },
    "color_contrast": { "score": 0, "evidence": "..." },
    "component_consistency": { "score": 0, "evidence": "..." },
    "iconography_detail": { "score": 0, "evidence": "..." },
    "primary_action": { "score": 0, "evidence": "..." },
    "empty_error_states": { "score": 0, "evidence": "..." }
  },
  "top_issues": [
    {
      "state": "state-name",
      "viewport": "1280",
      "expected": "what a careful interface would do",
      "actual": "what the screenshot shows"
    }
  ],
  "notes": "anything the scores do not capture, including screens you could not read"
}
```

Then write that exact object to `{{ANSWER}}` with your file writing tool. Write the whole file in one write, so a long evidence string is never cut off. Do not use a shell heredoc: it truncates long answers. Change no other file.
