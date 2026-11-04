# Design review: one category, blind pair

Two interfaces, labeled A and B. The labels are shuffled per category, and the
products are not named. Do not try to work out which product is which, and do
not read anything except this prompt, the rubric, and the images listed below.

Read the rubric at `{{RUBRIC}}` first. This call judges one category:
**{{CATEGORY_LABEL}}** (`{{CATEGORY}}`).

## Screens

Each directory holds one product at two window widths. The two panels are not
the same kind of app and are not the same screen; judge the category's craft,
not what the app is for.

{{POOLS}}

## Rules

- Pick exactly one winner: `A`, `B`, or `tie`.
- A win needs a citation from the winning panel: one specific, checkable
  detail. A measured distance, a spacing decision, a type-scale move, a
  control's exact height or weight, a alignment of two edges. Name the panel
  file and the value, so a person can open that image and check it.
- A category with no such citation is a tie. A tie is not a win: write what is
  equal, and what neither panel gets right.
- Do not reward a panel for having more images. Both sides have the same count.
- There is no score to hit and no expectation about which side should win. Do
  not mention scores, roadmaps, or what either product ought to become.
- Judge the interface. Browser chrome, window shadows and the capture
  environment are not part of the product.
- Budget: {{IMAGE_COUNT}} images, one reply. Do not spawn subagents, helpers or
  additional agents, and do not run any application. A call that spawns a
  helper is a failed call.
- If the images do not show enough to decide the category, return a tie and say
  what is missing.

## Output

Return one JSON object in a ```json fence and nothing else:

```json
{
  "role": "comparative",
  "round": {{ROUND}},
  "category": "{{CATEGORY}}",
  "winner": "A",
  "citation": "A/01.png: the two-line block is set at 13/20 with 6px between rows, and B/01.png sets the same block at 14/22 with 10px, so A's block reads as one unit.",
  "why": "one or two sentences comparing the category across both panels",
  "confidence": "clear"
}
```

`winner` is `A`, `B` or `tie`. `confidence` is `clear` or `slight`. Both
`citation` and `why` are required and may not be empty. Write that exact object
to `{{ANSWER}}` with a heredoc. Change no other file.
