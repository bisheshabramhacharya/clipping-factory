# Clipping Factory — Design System (v2)

> Apple-elegant redesign. Generated via gstack design consultation + taste skills
> (design-taste-frontend, high-end-visual-design, minimalist-ui).
> Variant exploration: `~/.gstack/projects/codingwithb-clipping-factory/designs/studio-v2-20250816/`

## Memorable thing

The clips are the product. The studio feels like **Apple made the tool a podcaster
uses**: calm, precise, expensive in its restraint. Nothing cheap, nothing loud.

## Product flow (three beats, not one page)

The studio is **three screens**, never a single scroll:

1. **Upload** — one centered drop zone, framing + caption choices up front.
2. **Processing** — a quiet progress rail; **clips stream in underneath it** as each
   finishes rendering.
3. **Results** — when done, the progress block **collapses into a status pill**
   ("Done · 6 clips · 4m 12s"); only the clips remain. Cancel/error stays reachable
   via that pill until the pipeline fully ends.

Routing is real (refresh/back safe). The review theater is an **overlay** on Results.

## Color

Apple's restraint: warm-neutral greys, near-black ink, one blue.

| Token | Hex | Use |
|---|---|---|
| `--bg` | `#F5F5F7` | App background |
| `--surface` | `#FFFFFF` | Cards, sheets |
| `--ink` | `#1D1D1F` | Primary text, primary controls |
| `--ink-2` | `#6E6E73` | Secondary text |
| `--hairline` | `rgba(0,0,0,0.08)` | Borders — hairline, never 1px hard gray |
| `--blue` | `#0071E3` | The one accent: primary action, active, links |
| `--blue-dark` | `#0A84FF` | Dark mode accent |
| `--ok` | `#34C759` | Keep / success (iOS green) |
| `--bad` | `#FF3B30` | Errors (iOS red) |
| `--shadow-soft` | `0 20px 60px rgba(0,0,0,0.08)` | Ambient, diffused — never harsh |

Dark variant: `#0A0A0C` background, `rgba(255,255,255,0.05)` surfaces,
`rgba(255,255,255,0.12)` hairlines, white primary text.

The **clip accent palette** (`#FFDD00`, `#7CFF4F`, `#FF4F4F`, `#4FB5FF`, `#C77DFF`,
`#FF9F1C`) belongs to the videos only. UI chrome is blue-on-neutral; the clips are
the color on the page.

## Typography

- **UI**: SF Pro — `system-ui, -apple-system` (macOS renders SF). Never Inter,
  Roboto, Arial, or Helvetica. Tight tracking on display (`-0.02em`), 17px base.
- **Display (optional variant)**: Apple's New York serif (`"New York", Georgia`),
  for editorial headlines only.
- **Meta**: 11px, `+0.06em` tracking, `#6E6E73` — ranks, timecodes, eyebrows.
- **Captions (in clips)**: Impact = heavy stacked lockups, white + black outline;
  Clean = light sans, 3–7 word groups, amber. Untouched by chrome.

## Components

- **Buttons**: full-round pills (`border-radius: 980px`), generous padding.
  Primary = blue fill; the arrow/play glyph nests inside its own circular inset
  ("button-in-button"). Active press: `scale(0.98)`.
- **Cards**: white surface, hairline border, 20px+ radius, `--shadow-soft`.
  Double-bezel on hero elements: outer shell (hairline + padding) around an inner
  core with its own inset highlight.
- **Eyebrows**: 11px uppercase micro-labels above headlines.
- **Drop zone**: giant rounded rectangle, dashed hairline, gentle darkening on
  hover/drag — the single most important object on the upload screen.

## Spacing

Macro-whitespace is the luxury. Sections breathe: `padding: 64–96px` vertical,
`max-width: 1080px`. Clip cards: 140px 9:16 thumbnail | copy | actions, 24px gaps.

## Motion

- One curve everywhere: `cubic-bezier(0.32, 0.72, 0, 1)`, 500–700ms.
- Entrances: fade-up + blur resolve (`translateY(16px) blur(4px) opacity 0` →
  settled), staggered per clip as they stream in.
- Collapse (progress → pill): animates `transform/opacity`/`max-height` only —
  never layout-triggering properties.
- All animation GPU-safe: `transform` + `opacity` only. No `linear`, no `ease-in-out`.
- Respect `prefers-reduced-motion`; the clips themselves never animate.

## Voice

Short, factual, no hype. "One podcast in. A few faithful clips out." Muted
one-liners explain each state; the UI never lectures.

## Anti-cheap checklist (from high-end-visual-design)

- No Inter/Roboto/Arial/Helvetica as the face
- No 1px solid gray borders, no harsh shadows
- No AI-purple gradients, no decorative blobs, no 3-column icon grids
- No edge-to-edge sticky bars; header floats as a glass pill
- No `linear` transitions; nothing appears statically
- Every card is double-bezel; every CTA is a pill
