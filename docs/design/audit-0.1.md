# Design audit of Loci 0.1.0

Captured on 2026-10-11 from the v0.1.0 build: the released AppImage under Xvfb (first launch,
GTK light and dark), and every screen of the same interface through the capture harness
(headless Chrome against `loci serve`), at 1440×900, 1024×768, 390×844 and 360×780, light and
dark: 65 screens, 392 images. Every finding below was seen in a capture; the file names in
brackets are the harness names (`<screen>-<w1440|w1024|w390|w360>-<light|dark>.png`).

The verdict first: the design contract in `DESIGN.md` §17 is largely implemented. Tokens are
OKLCH, colours never appear as hex outside `tokens.css`, both themes are designed rather than
inverted, logical properties are used throughout, menus, sheets, dialogs, tabs, switches and the
note-type picker are all custom. No screen overflows horizontally at 360 px. What remains is the
gap between "correct" and "finished": a handful of native controls, a loud chart palette, a
study screen that is calm but not yet beautiful, an accent that turns into a highlighter in the
dark theme, and a window that knows nothing about the OS it runs in.

## 1. What reads as generic or unfinished

1. **The study screen is a box in a field** (`study-front-w1440-*`, `study-back-w1440-*`,
   `study-cloze-back-w1440-light`). The card panel has a fixed `min-block-size: min(16rem, 40vh)`,
   so a one-word Arabic card sits in 260 px of empty paper; the panel floats in the centre of a
   flat background with nothing to anchor it. In dark the paper (`--surface` 21 %) is barely
   distinguishable from the desk (`--bg` 17 %). The "desk and paper" of the identity is not yet
   visible.
2. **Zero counts render as dashes in the study top bar** (`study-cloze-front-w1440-light`:
   "7 – –"). On Home the same dashes work inside a table; in a 44 px bar they read as missing data.
3. **Numerals have no system.** Intervals under the grade buttons, the three counts, the deck
   table, the import tiles (`import-preview-w1440-light`, Newsreader figures next to Inter labels)
   and the card-info panel each pick a face ad hoc. The brief asks for JetBrains Mono tabular
   numerals for counts and intervals; `--font-mono` is used in 15 places, none of them a count.
4. **Primary buttons in dark are pale yellow slabs** (`home-w1440-dark`, `backlog-w1440-dark`,
   `add-io-empty-w1440-dark`, `home-custom-session-w1440-dark`). `--accent-strong` in dark is
   oklch 84 % lightness, designed for text on slate, then reused as a fill with near-black text.
   It is the one element on the page that looks like a different product.
5. **Charts use Anki's palette, not Loci's** (`stats-w1440-dark`, `stats-w1440-light`). Forecast
   is blue on green, stability is sky blue, true-retention bars are green and orange, answer
   buttons are four saturated hues. Restyle on the existing chart code: brass as the single series
   colour, slate greys for secondary series and reference lines; grade colours stay only where the
   bar *is* a grade.
6. **Dialog and insight icons sit in tinted circles** (`export-dialog-w1440-light`,
   `study-help-w1440-light`, `home-w1440-*`): a generic "app" motif that adds nothing. Replace
   with a bare 20 px icon aligned to the title, or drop it.
7. **First launch says "browser"** (`welcome-w1440-*`, `settings-data-w1440-light`,
   `about-w1440-*`): "Your cards stay in this browser", "Your collection lives in this browser",
   "Storage: This browser only", and the About link points at the TypeScript repository. The
   first-run screen also lacks the "open the manual" path the brief asks for.
8. **Empty states and the session summary are correct but plain** (`study-empty-w1440-light`,
   `study-summary-w1440-light`): a tinted circle, a serif line, three buttons.

## 2. Where native controls show

Counted by the harness (`findings.json`, `nativeControls`), then confirmed in the images:

| Control | Where | Count | What it looks like |
|---|---|---|---|
| `<select>` | Import preview (merge rule), Settings (day start, preset), Preset editor (8), Notetypes | 1 · 2 · 8 | OS popup list; and the chevron icon is drawn *over the start* of the value ("⌄Newest edit wins", `import-preview-w1440-light`; "⌄4 AM", `settings-w390-dark`; every select in `preset-editor-w1440-light`) |
| `<input type="radio">` | Custom session sheet (5) | 5 | grey OS radio circles (`home-custom-session-w1440-dark`) |
| `<input type="range">` | Settings (4), Backlog (1), Preset editor (3) | 8 | styled for WebKit only; Firefox and the Android webview fall back to the default thumb |
| `<input type="file">` | Welcome, Import | 1 | visually hidden; fine |
| `title=""` tooltips | 139 attributes across 12 views (Stats 24, BulkBar 14, Home 22 on one screen) | | OS tooltip bubbles, delayed and unstyled; the `Tooltip` primitive exists and is used in a few places |
| `<textarea>` | Template editor | 1 | styled; fine |
| scrollbars | everywhere | | `::-webkit-scrollbar` styled; `scrollbar-color` set for Firefox; acceptable |

## 3. Where the identity is broken

- "Never glassmorphism": the tablet top bar and the phone tab bar use `backdrop-filter: blur()`
  (`AppShell.module.css` lines 66 and 86). Make them opaque surfaces with a hairline.
- "A single brass accent": the chart palette (above) and the four-colour grade legend in the
  session summary dilute it; Anki's count colours (new blue, learning orange, due green) are the
  contract's deliberate exception and stay.
- "Deep slate at night": the dark surfaces are right; the dark accent fill is not (finding 4).

## 4. Where dark mode is incomplete

- The window itself: the Tauri window never receives the theme, so on macOS and Windows the title
  bar, native dialogs and menus follow the OS while the content follows the in-app preference;
  on Linux the WebKitGTK `prefers-color-scheme` is whatever GTK reports at launch (both the GTK
  light and the GTK dark run under Xvfb came up dark, `native/first-launch-*.png`).
- Inside the card shadow root the defaults are sound (`study-*-w1440-dark`): ink, hairline,
  cloze colour, typed-answer diff and the image-occlusion masks all read correctly. The
  image-occlusion mask fill (`line-strong` 72 % over surface) is the one weak spot: in dark it is
  a grey block on a dark card and the active mask's brass outline is the only cue.
- Selected rows in Browse use `--accent-soft` (`browse-w1440-dark`): a brown that reads as
  "stale", not "chosen". Use a slate selection with a brass leading rule instead.

## 5. Where spacing and type drift

- Ten `font-size: Npx` declarations outside the scale (charts 11 px, heatmap 9 px, keycaps 10/11
  px, deck-tree head 10 px, preset chips 11 px). Fold them into `--text-2xs: 0.6875rem` (11) and
  `--text-3xs: 0.625rem` (10); 9 px is below the floor.
- The study top bar is 44 px on desktop and 52 px on touch, the sheet header 64/56, the browse
  header row 34, the tablet top bar 60: five chrome heights where two would do (44 and 56).
- Card column padding uses `clamp(16px, 6vh, 64px)` top but a fixed 40 px bottom, so the card is
  optically high on tall windows and low on short ones.
- Home at 1024 keeps the 220 px sidebar and a single insight column; the deck table's six columns
  are tight but hold. At 390 the hero sentence breaks "About 21 min / to clear today." which is
  the right break.
- On phones the deck overflow menu opens over the tab bar and its Delete item sits on top of
  the Add button (`home-deck-menu-w390-dark`); menus must stay inside the safe area above the bar.
- Arabic text reaches three different stacks: `"Noto Naskh Arabic", serif`, `"Noto Naskh Arabic",
  Vazirmatn, serif` and Naskh prepended to the Newsreader stack; and a few labels fall back to a
  bare `system-ui, sans-serif` (harness `fonts` list). One `--font-arabic` and no bare fallbacks.
- Phone study (`study-back-w390-dark`): the answer sits in the upper third and the lower two
  thirds are empty down to the buttons; the grade buttons are 64 px tall and the explainer wraps to
  two lines under them. The reach is right; the proportion is not.

## 6. Window and chrome

- No application menu on any platform. On macOS this means no Edit menu, so Cmd+C/V/X in the
  webview depend on WebKit's defaults, and no Cmd+Q, Cmd+W, Cmd+M, Window or Help menus.
- Window size and position are not remembered; every launch is 1280×820 centred.
- The icon set (`src-tauri/icons/`) is the Tauri default square, not the Loci mark.
- No command palette; the theme can only be changed in Settings or by the three-state toggle in
  the sidebar footer.

The 100,000-card browse (`browse-100k-w1440-*`, `browse-100k-row50k-w1440-light`) scrolls and
jumps without a hitch; the table itself is sound and only needs the header, selection and bulk
bar brought up to the identity.

## 7. What is already right and must not be lost

The home hero sentence; the insight strip; the momentum heatmap; the card-info panel; the
note-type combobox; the tag input; the add sheet; the catch-up screen's text; the quiet toast;
the import preview's deck and note-type lists; the destructive-dialog pattern; the keyboard-hint
caps; reduced motion; RTL mirroring. The fixes below are additive.

## 8. What this round changes (summary of the plan)

System: dark accent fill token; `Select`, `Radio`, `Checkbox` primitives with designed popups and
marks; `Slider` thumb for every engine; `Tooltip` replaces every `title`; numeral classes
(`.num`, `.num-lg`) on JetBrains Mono tabular; two `--text-2xs/3xs` steps; opaque bars; chart
palette. Study: desk and paper, panel sized by content, counts never dashed, intervals in mono,
hints that fade after the first grades, phone proportions. Shell: window theme from the
preference (and `set_theme` on change), native menu on macOS, command palette (Ctrl/Cmd+K) with
theme commands, remembered window state, the Loci mark as the icon set, first-launch copy and the
manual card. Browser and stats: table header and selection restyle; charts in the identity.
Results in `docs/design/contrast-0.2.md` and the new captures in `docs/captures/`.
