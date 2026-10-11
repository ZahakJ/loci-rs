# Contrast of the Loci tokens, 0.2

Generated 2026-10-11 by `apps/loci/scripts/contrast.ts` (`pnpm contrast`) from `src/ui/design/tokens.css`.
Colours are converted from OKLCH to sRGB by the script (OKLab → linear sRGB, clipped to gamut); ratios are WCAG 2.x.

Thresholds: 4.5 for text (including count numerals and pill labels); 3.0 for large text, UI components and the
grade colours, which appear as 2 px rules and legend dots, never as text. `faint` is decorative and listed for information only.
Pill text is `color-mix(in oklab, tone, ink)` over `color-mix(in oklab, tone, bg)`, with the percentages read from `Pill.module.css`.

| Theme | Pair | Role | Ratio | Needs | Result |
|---|---|---|---:|---:|---|
| light | ink / bg | body text | 16.11 | 4.5 | PASS |
| light | ink / surface | body text on panels | 16.82 | 4.5 | PASS |
| light | ink-2 / bg | secondary text | 10.10 | 4.5 | PASS |
| light | muted / bg | muted text | 5.58 | 4.5 | PASS |
| light | muted / surface | muted text on panels | 5.83 | 4.5 | PASS |
| light | muted / surface-2 | muted text on hover / inset | 5.26 | 4.5 | PASS |
| light | faint / bg | decorative (informational) | 2.89 | 3.0 | below (informational) |
| light | accent-strong / bg | accent text, links | 4.83 | 4.5 | PASS |
| light | accent-strong / surface | accent text on panels | 5.05 | 4.5 | PASS |
| light | on-accent-fill / accent-fill | primary button label | 5.20 | 4.5 | PASS |
| light | on-accent-fill / accent-fill-hover | primary button label, hover | 6.15 | 4.5 | PASS |
| light | ink / accent-soft | text on a selection tint | 14.48 | 4.5 | PASS |
| light | on-accent / accent-strong | text on accent-strong | 5.20 | 4.5 | PASS |
| light | danger / bg | error text | 4.77 | 4.5 | PASS |
| light | new / bg | new count (text) | 4.71 | 4.5 | PASS |
| light | learn / bg | learning count (text) | 4.79 | 4.5 | PASS |
| light | due / bg | due count (text) | 4.81 | 4.5 | PASS |
| light | again / surface | grade rule, legend dot | 3.85 | 3.0 | PASS |
| light | hard / surface | grade rule, legend dot | 3.13 | 3.0 | PASS |
| light | good / surface | grade rule, legend dot | 3.20 | 3.0 | PASS |
| light | easy / surface | grade rule, legend dot | 3.18 | 3.0 | PASS |
| light | pill new | pill text (new 68% ink / 13% bg) | 6.17 | 4.5 | PASS |
| light | pill learn | pill text (learn 68% ink / 13% bg) | 6.23 | 4.5 | PASS |
| light | pill due | pill text (due 68% ink / 13% bg) | 6.23 | 4.5 | PASS |
| light | pill neutral | pill text (muted 68% ink / 13% bg) | 6.81 | 4.5 | PASS |
| light | pill accent | pill text (accent 68% ink / 13% bg) | 5.12 | 4.5 | PASS |
| light | pill again | pill text (again 68% ink / 13% bg) | 5.30 | 4.5 | PASS |
| light | pill hard | pill text (hard 68% ink / 13% bg) | 4.65 | 4.5 | PASS |
| light | pill good | pill text (good 68% ink / 13% bg) | 4.72 | 4.5 | PASS |
| light | pill easy | pill text (easy 68% ink / 13% bg) | 4.71 | 4.5 | PASS |
| dark | ink / bg | body text | 15.56 | 4.5 | PASS |
| dark | ink / surface | body text on panels | 14.42 | 4.5 | PASS |
| dark | ink-2 / bg | secondary text | 10.95 | 4.5 | PASS |
| dark | muted / bg | muted text | 6.64 | 4.5 | PASS |
| dark | muted / surface | muted text on panels | 6.15 | 4.5 | PASS |
| dark | muted / surface-2 | muted text on hover / inset | 5.55 | 4.5 | PASS |
| dark | faint / bg | decorative (informational) | 3.47 | 3.0 | PASS |
| dark | accent-strong / bg | accent text, links | 11.59 | 4.5 | PASS |
| dark | accent-strong / surface | accent text on panels | 10.74 | 4.5 | PASS |
| dark | on-accent-fill / accent-fill | primary button label | 4.70 | 4.5 | PASS |
| dark | on-accent-fill / accent-fill-hover | primary button label, hover | 5.56 | 4.5 | PASS |
| dark | ink / accent-soft | text on a selection tint | 11.17 | 4.5 | PASS |
| dark | on-accent / accent-strong | text on accent-strong | 11.00 | 4.5 | PASS |
| dark | danger / bg | error text | 6.14 | 4.5 | PASS |
| dark | new / bg | new count (text) | 7.75 | 4.5 | PASS |
| dark | learn / bg | learning count (text) | 9.20 | 4.5 | PASS |
| dark | due / bg | due count (text) | 8.12 | 4.5 | PASS |
| dark | again / surface | grade rule, legend dot | 6.16 | 3.0 | PASS |
| dark | hard / surface | grade rule, legend dot | 9.28 | 3.0 | PASS |
| dark | good / surface | grade rule, legend dot | 8.09 | 3.0 | PASS |
| dark | easy / surface | grade rule, legend dot | 8.45 | 3.0 | PASS |
| dark | pill new | pill text (new 68% ink / 13% bg) | 8.44 | 4.5 | PASS |
| dark | pill learn | pill text (learn 68% ink / 13% bg) | 9.24 | 4.5 | PASS |
| dark | pill due | pill text (due 68% ink / 13% bg) | 8.63 | 4.5 | PASS |
| dark | pill neutral | pill text (muted 68% ink / 13% bg) | 7.74 | 4.5 | PASS |
| dark | pill accent | pill text (accent 68% ink / 13% bg) | 9.35 | 4.5 | PASS |
| dark | pill again | pill text (again 68% ink / 13% bg) | 7.75 | 4.5 | PASS |
| dark | pill hard | pill text (hard 68% ink / 13% bg) | 9.68 | 4.5 | PASS |
| dark | pill good | pill text (good 68% ink / 13% bg) | 8.98 | 4.5 | PASS |
| dark | pill easy | pill text (easy 68% ink / 13% bg) | 9.21 | 4.5 | PASS |

Every required pair passes in both themes.
