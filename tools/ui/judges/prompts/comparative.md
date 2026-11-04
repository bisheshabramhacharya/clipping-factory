# Design review: comparative pass

You are comparing screenshots of four different products. The directories are
labeled A, B, C and D; the labels are shuffled and the products are not named.
Do not try to guess which product is which.

Read the rubric at `{{RUBRIC}}` first. It defines eight categories and the
anchors for each score.

## Screens

Each directory holds published screenshots from one product. Read every image
in every directory.

{{POOLS}}

## Rules

- Score each source independently on all eight categories. Whole numbers 1 to 10.
- A source's score is a judgment about that product's interface as a whole, not
  an average over its files. Cite specific files (`A/03.png`) as evidence.
- Do not reward a product for having more screenshots. Score the quality of
  what is visible.
- Judge the interface, not the content of the footage. Browser chrome and
  operating-system window frames are not part of the product.
- No score is expected to be high or low.
- Do not mention targets, roadmaps, or what any product ought to become.
- Do not run any application or test. Do not modify any file except the answer file below.

## Output

Return one JSON object in a ```json fence and nothing else:

```json
{
  "role": "comparative",
  "round": {{ROUND}},
  "sources": {
    "A": {
      "categories": {
        "typography": { "score": 0, "evidence": "A/01.png: ..." },
        "hierarchy": { "score": 0, "evidence": "..." },
        "spacing_rhythm": { "score": 0, "evidence": "..." },
        "color_contrast": { "score": 0, "evidence": "..." },
        "component_consistency": { "score": 0, "evidence": "..." },
        "iconography_detail": { "score": 0, "evidence": "..." },
        "primary_action": { "score": 0, "evidence": "..." },
        "empty_error_states": { "score": 0, "evidence": "..." }
      }
    },
    "B": { "categories": { "...": { "score": 0, "evidence": "..." } } },
    "C": { "categories": { "...": { "score": 0, "evidence": "..." } } },
    "D": { "categories": { "...": { "score": 0, "evidence": "..." } } }
  },
  "pairs": [
    {
      "higher": "A",
      "lower": "C",
      "category": "hierarchy",
      "note": "where C loses to A, on which screen, and why",
      "screens": ["C/02.png", "A/01.png"]
    }
  ],
  "notes": "anything the scores do not capture"
}
```

For every unordered pair of sources (six pairs), include at least one `pairs`
entry naming the category with the clearest visible gap and where the lower
source loses. A pair may have more than one entry if the gap is clear in
several categories.

Then write that exact object to `{{ANSWER}}` with a heredoc. Change no other
file.
