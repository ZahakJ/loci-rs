# Loci design system

`tokens.css` is the visual contract (written by the architect; change only with a reason
recorded in DESIGN.md §17). Everything else in this folder implements it.

## Rules
- Colours only via tokens. Never hex in components.
- Borders over shadows in light; `--shadow-2` only for floating sheets/menus.
- Type: UI in `--font-ui` at `--text-md`; headings `--text-xl/2xl` weight 600, letter-spacing `--tracking-tight`; numbers `font-variant-numeric: tabular-nums`.
- Spacing on the 4-point grid (`--s-*`). Tap targets ≥ `--tap`.
- Motion via `--d-*` and `--ease`; every animation must be invisible when `--d-base` is 0.
- Logical properties only (`margin-inline`, `padding-block`, `inset-inline-start`); the UI mirrors under `dir="rtl"`.
- Focus: `outline: 2px solid var(--focus); outline-offset: 2px;` on `:focus-visible`, never removed.
- Icons: inline SVG, 20px, 1.5px stroke, `currentColor`. Keep an `icons.tsx` map; no icon font.

## Primitives (`primitives/`)
`Button` (variants: primary · secondary · ghost · danger; sizes sm · md · lg; `kbd` hint prop),
`IconButton`, `Kbd`, `Pill` (tone: new · learn · due · neutral · accent · again · hard · good · easy),
`Card` (surface panel), `Sheet` (side sheet on desktop, bottom/full sheet on phone; focus trap; Esc closes),
`Dialog` (confirm; destructive variant), `Menu` (overflow menu with keyboard nav), `Field`
(label + control + help + error), `Input`, `Select`, `Switch`, `Slider` (with value label),
`Segmented`, `Tabs`, `Tooltip` (also for touch via long-press), `Toast` (live region),
`EmptyState`, `ProgressBar`, `Skeleton`, `Heatmap` (12 months, 4 steps of `--accent`),
`Sparkline`. All accept `className`; none use inline colours.
