# Design guidelines (Apple HIG, macOS desktop density)

Rules an agent must follow so a new screen looks like it belongs. Components and class names are in [README.md](./README.md).
When unsure, copy the nearest pattern from `gallery/DesignGallery.tsx` or from macOS **System Settings**.

## 1. Principles

1. **Clarity** — one primary action per view; plain words; no decoration that carries no meaning.
2. **Deference** — content first. Chrome (sidebar, toolbar) is translucent and quiet; colour is for state and the primary action.
3. **Depth** — layers are communicated with materials and elevation tokens, not borders everywhere.
4. **Native feel on every OS** — system font stack, platform-correct key glyphs (⌘ on macOS, Ctrl/Win elsewhere), real window controls.

## 2. Typography

Font: `var(--font)` (`-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable", "Inter", system-ui`). Never import a web font.

| Style (class)               | Size/line | Weight | Use for                                                           |
| --------------------------- | --------- | ------ | ----------------------------------------------------------------- |
| Large Title `t-large-title` | 26/32     | 700    | One hero line per full-window page (onboarding, "You're all set") |
| Title `t-title`             | 22/26     | 700    | Page title in the toolbar (`<h1>`), status-card headline          |
| Title 3 `t-title3`          | 15/20     | 600    | Sheet titles, empty-state titles, stat numbers' labels in cards   |
| Headline `t-headline`       | 13/16     | 600    | Row emphasis, popover headers                                     |
| Body `t-body`               | 13/16     | 400    | Default text, row titles                                          |
| Callout `t-callout`         | 12/15     | 400    | Supporting sentences, banner details                              |
| Footnote `t-footnote`       | 11/14     | 400    | Section titles/footers, row subtitles, captions                   |

Rules: never go below 11 px. Max two weights per view (regular + semibold). Secondary text uses `t-secondary` (`--label-secondary`), never opacity. Numbers that change (latency, counts) use the normal font; only codes/commands use `--font-mono`. Sentence case everywhere ("Add computer…" is acceptable only for menu-like commands; headings use sentence case).

## 3. Spacing and shape

- 4 pt grid; layout rhythm is 8 pt. Use only `--s-1…--s-12` (4, 8, 12, 16, 20, 24, 32, 40, 48).
- Page gutters 24 px (`--s-6`); content column max 780 px, centred; cards padded 16 px; rows min-height 44 px.
- Gaps: 8 between related controls, 16 between cards, 24 between sections.
- Radii: controls 6, grouped lists 10, cards 12, sheets 14, pills 999. Don't invent new radii.
- Hit targets ≥ 24 px tall (28 regular, 36 large). Icon-only buttons are 28 × 28 minimum with an `aria-label` and a Tooltip.

## 4. Colour semantics

- Text: `--label` (primary), `--label-secondary` (supporting, 4.5:1+), `--label-tertiary` (disabled/decorative only — never for information).
- Surfaces: window `--bg-window`; grouped rows/cards `--bg-secondary`; wells and fills `--bg-tertiary`.
- **Accent blue** is for the single primary action, selection (sidebar item, focused/selected card) and links. Use `--accent` for fills with white text, `--accent-text` for blue text.
- **Status colours carry meaning only**: green = healthy/connected, orange = in-progress/attention, red = error/needs action, gray = off/unavailable. Dots/icons use `*-fill`, text uses the plain colour, backgrounds use `*-soft`. Never rely on colour alone: pair it with text or an icon.
- Contrast: body text ≥ 4.5:1 in both themes. Don't put white text on `*-fill` backgrounds smaller than 18 px unless it is `--accent`/`--red-fill` buttons already styled by the components.
- Never hard-code hex values in screens; both themes must work (test light, dark, and forced `data-theme`).

## 5. Layout patterns

**Main window (macOS System Settings)**: `[Sidebar 220 px | Main]`. Sidebar: items with thin-stroke icons (16 px), the active item filled with accent; connection status chip at the bottom. Main: `Toolbar` (title + at most one trailing action) → optional banner region → scrolling content column.

**Inset grouped list**: `Section` (small secondary title above, optional footnote below explaining consequences) containing `Row`s. One idea per row; title left, control right (`Toggle`, `Select`, `SegmentedControl`, `Button size="small"`, `Badge`). Put long explanations in the footer, not in the row.

**Card**: `.fl-card` for non-list content (status, diagram, stats). Never nest cards.

**Sheet** (modal task, e.g. "Add Computer"): title (Title 3, centred), body, footer buttons right-aligned, primary last. Esc dismisses; the primary action is the default.

**Alert** (confirmation): title = the question ("Forget Office PC?"), message = the consequence in one sentence, stacked full-width buttons: destructive action first and red, **Cancel last and focused**.

**Banner**: top of the content column, one per state, title + one short sentence + at most one action ("Try Again", "Allow…"). Banners describe the _system_ state; problems with one item are an inline `.fl-notice` under that item's row.

**HUD**: centred 200 px square for momentary confirmations that need no input (control moved to another computer). Shows 1.5–2 s, `role=status`, never takes focus.

**Toast**: bottom-centre pill for results of an action ("Shortcut updated"). Errors use `toast.show(msg, 'error')` and stay longer.

**Popover / tray (≈360 × 480)**: compact header (name + status dot), list of choices as menu-like rows (hover = accent fill), primary quick action, footer with 2 buttons.

**Onboarding**: full window, one decision per page, big illustration (inline SVG, `currentColor`), one primary button, progress dots at the bottom, Back at the top-left, "Skip for now" as a plain button when a step is optional.

**Key shortcuts** are drawn as `KeyCombo` key caps, never as plain text like "Ctrl+Shift+K". Recording UI uses `ShortcutRecorder`.

## 6. Copy tone

- Friendly, concise, plain language. Talk to the person ("your keyboard and mouse"), not about the system.
- **Never** show jargon or internals: no "daemon", "IPC", "socket", "token", "baton", "payload", error codes, stack traces or raw exception text. Map machine error codes to friendly sentences in one place.
- Buttons are verbs ("Add Computer…", "Switch to Office PC", "Try Again"). Use an ellipsis only when the action opens another step. Avoid "OK"/"Yes"/"No".
- Say what happened and what to do next: "Flow can't reach Office PC. Make sure it's on and on the same network." Not "Connection failed (E_TIMEOUT)".
- Explain consequences of destructive actions in the alert message. Never blame the user.
- Use the product name once per surface at most; don't repeat the screen title in body text.
- Sentence case, curly apostrophes (’), no exclamation marks, no emoji in UI text.

## 7. States every screen must handle

| State                      | What to render                                                                                                                                 |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Loading / connecting**   | `Spinner` centred (with `label`), no layout jump; skeleton is optional. Disable, don't hide, controls awaiting a result (`loading` on Button). |
| **Empty**                  | `EmptyState`: icon/illustration, one-sentence title, one-sentence explanation, one primary action. Never an empty list or "No data".           |
| **Error (action)**         | Friendly toast (`'error'`) or inline `.fl-notice` with an actionable sentence; keep the user's input.                                          |
| **Error (system)**         | `Banner` with a recovery action; the rest of the UI stays visible but dimmed controls are disabled.                                            |
| **Offline / disconnected** | Gray/orange banner with "Try Again"; show last-known data with relative time ("seen 12 minutes ago").                                          |
| **Permission missing**     | Red banner + a sheet with per-OS steps and deep links; never a dead end — always offer "Not Now".                                              |
| **Success**                | Toast or HUD for transient confirmation; a persistent success state only at the end of a flow (checkmark circle + "Done").                     |
| **Destructive**            | Alert with the safe action focused.                                                                                                            |

Also handle: very long names (truncate with ellipsis, keep full text in a tooltip/aria-label), 0/1/many items, light/dark, narrow widths (sidebar collapses at 720 px), and slow commands (keep the button busy ≥ the command's duration).

## 8. Accessibility contract

- Use semantic elements first: `button`, `a`, `input`, `select`, `ul/li`, `h1…h3`. Custom widgets must expose roles: switch (`role=switch`, `aria-checked`), segmented (`radiogroup`/`radio`), modal (`role=dialog|alertdialog`, `aria-modal`, labelled), banners (`role=status|alert`), HUD/toast (live regions).
- Every control has an accessible name. Icon-only buttons need `aria-label` (+ Tooltip). Decorative icons get `aria-hidden`.
- **Keyboard**: everything reachable and operable with Tab/Shift+Tab, Space/Enter, Arrow keys inside groups; sidebar arrows move between items; modals trap focus, close on Esc and return focus to the opener; shortcuts for primary navigation (⌘/Ctrl + 1…5).
- **Focus**: visible 3 px ring (`:focus-visible` → `--focus-ring`); never `outline: none` without replacement.
- **Contrast**: ≥ 4.5:1 for text, ≥ 3:1 for UI boundaries/icons that convey meaning, in light and dark.
- **Motion**: only transitions via `var(--dur*)` so `prefers-reduced-motion` removes them; no auto-playing animation except spinners/pulses that signal progress.
- Status is never colour-only; key caps have spoken labels (`spokenKeys`); live regions announce transient confirmations politely, errors assertively.
- Test with keyboard only, a screen reader pass on one screen, light/dark, and 200% zoom.

## 9. Do / Don't

| Do                                                               | Don't                                                                |
| ---------------------------------------------------------------- | -------------------------------------------------------------------- |
| Use tokens (`var(--s-4)`, `var(--label-secondary)`)              | Hard-code px/hex values or new radii                                 |
| One primary (accent) button per view                             | Several blue buttons competing                                       |
| Inset grouped lists with footers that explain                    | Free-floating label/value tables, borders on every row               |
| Pair status dots with text                                       | Colour-only status                                                   |
| Confirm destructive actions with the safe option focused         | Delete on a single click, or focus the destructive button            |
| Show friendly, actionable errors                                 | Show codes, stack traces or "Something went wrong" with no next step |
| Keep banners for system state, notices for item state            | Duplicate the same problem in banner + notice                        |
| Use `KeyCombo` for shortcuts                                     | Write "Ctrl+Shift+K" as text or hard-code ⌘ for all OSes             |
| Design empty, loading, error, offline states first               | Ship the happy path only                                             |
| Honour light, dark, system and forced themes                     | Assume light mode                                                    |
| Keep copy short and human                                        | Use "daemon", "IPC", "baton", "payload", "sync failed"               |
| Animate with `--dur`/`--ease`                                    | Add bounce/parallax or long animations                               |
| Truncate long names with ellipsis + full text in `title`/tooltip | Let text wrap and break rows                                         |
