# Loci design system

`tokens.css` holds the visual tokens (`docs/internals.md` §15). Everything else in this folder
builds on it.

## Rules
- Colours only via tokens. Never hex in components. Filled controls (primary buttons, switches, checked marks) use `--accent-fill` / `--on-accent-fill`; `--accent-strong` is for accent text, icons and rules only. `pnpm contrast` checks every pair in both themes.
- Borders over shadows in light; `--shadow-2` only for floating sheets/menus.
- Type: UI in `--font-ui` at `--text-md`; headings `--text-xl/2xl` weight 600, letter-spacing `--tracking-tight`. Font sizes only from the scale (`--text-3xs` … `--text-3xl`), never `px`. Every family through `--font-ui`, `--font-card`, `--font-arabic` (the one Arabic stack) or `--font-mono`.
- Numerals: numbers the user reads as data (counts, intervals, dates, sizes, figures) are mono tabular: `.num` (0.9em) or `.num-lg` (1em, big figures). Numbers inside prose stay in the text face.
- No `title` attributes: explain with the `Tooltip` primitive (IconButton does it by default) or visible help.
- Floating layers (Menu, Select, Tooltip, Combobox) stay inside the viewport and above the bottom bar (`floorY()` from `bottomBar.ts`, `--bottom-bar-height` on `<html>`). No `backdrop-filter`.
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
(label + control + help + error), `Input`, `Select` (designed listbox, not the OS popup), `Radio`, `Checkbox`
(20 px marks over a visually hidden native input), `Switch`, `Slider` (with value label),
`Segmented`, `Tabs`, `Tooltip` (also for touch via long-press), `Toast` (live region),
`EmptyState`, `ProgressBar`, `Skeleton`, `Heatmap` (12 months, 4 steps of `--accent`),
`Sparkline`. All accept `className`; none use inline colours.
