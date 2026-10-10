# @flow/design-system

An Apple-HIG (macOS desktop density) design system: **design tokens**, **plain-CSS components** and **React components**.
It is a private package that ships TypeScript/CSS **source** (no build step). Read [GUIDELINES.md](./GUIDELINES.md) for the rules that
make screens look consistent; this file is the reference for _how to use_ the pieces.

```
design-system/
├── README.md             this file
├── GUIDELINES.md         HIG rules, layout patterns, copy tone, states, accessibility, do/don't
├── package.json          @flow/design-system (private, ESM, peer deps: react, react-dom, lucide-react)
├── tokens/
│   ├── tokens.css        CSS custom properties, light + dark + [data-theme] override, base reset, type roles
│   └── tokens.json       the same tokens as data (generated: `npm run tokens`)
├── styles/
│   └── components.css    every component as plain CSS classes (prefix `fl-`), no JS needed
├── src/                  React components, key helpers, theme provider, focus trap
├── gallery/              DesignGallery.tsx: living reference page of every component
└── scripts/build-tokens.mjs   tokens.css -> tokens.json
```

## Use it from React (Vite/webpack/Next with TS source support)

```jsonc
// package.json of the consuming app
{
  "dependencies": {
    "@flow/design-system": "file:../path/to/design-system",
    "react": "^18",
    "react-dom": "^18",
    "lucide-react": "*",
  },
}
```

```tsx
import {
  ThemeProvider,
  ToastProvider,
  Button,
  Section,
  Row,
  Toggle,
} from "@flow/design-system";
// Importing the package also loads tokens.css + components.css (side effects).

export function App() {
  return (
    <ThemeProvider>
      <ToastProvider>
        <Section title="Startup">
          <Row
            title="Open at login"
            trailing={
              <Toggle label="Open at login" checked onChange={() => {}} />
            }
          />
        </Section>
      </ToastProvider>
    </ThemeProvider>
  );
}
```

Bundler notes (Vite): add `resolve: { dedupe: ['react', 'react-dom', 'lucide-react'] }` and `server.fs.allow` for the package directory;
in `tsconfig.json` add `"include": [..., "../path/to/design-system/src"]` and
`"paths": { "*": ["./node_modules/@types/*", "./node_modules/*"] }` so the package resolves React types from the app.
Gallery page: `import { DesignGallery } from '@flow/design-system/gallery'`.

## Use it without React (any framework, plain HTML)

```html
<link rel="stylesheet" href="design-system/tokens/tokens.css" />
<link rel="stylesheet" href="design-system/styles/components.css" />
<body>
  <ul class="fl-list">
    <li class="fl-row-wrap">
      <div class="fl-row">
        <span class="fl-row__main"
          ><span class="fl-row__title">Open at login</span></span
        >
        <span class="fl-row__trailing"
          ><button
            class="fl-toggle"
            role="switch"
            aria-checked="true"
            aria-label="Open at login"
          ></button
        ></span>
      </div>
    </li>
  </ul>
  <button class="fl-btn fl-btn--prominent">Continue</button>
</body>
```

The CSS only styles; **you** supply behaviour and ARIA (see "Accessibility contract" in GUIDELINES.md). Toggle: flip `aria-checked`.
Segmented items: `aria-checked`. Sidebar active item: `aria-current="page"`.

Tokens as data (Figma/Style Dictionary/native apps): `tokens/tokens.json` has `font`, `typeScale`, `spacing`, `radii`, `motion`,
and `themes.light|dark.{colors,shadows}`. It is generated from `tokens.css`; edit the CSS, then run `npm run tokens`.

## Theming and dark mode

- Light is the default; dark follows `prefers-color-scheme`.
- Force a theme with `<html data-theme="light">` or `<html data-theme="dark">`; remove the attribute for "System".
- React: wrap in `<ThemeProvider>`; `useTheme()` returns `{ theme: 'system'|'light'|'dark', setTheme }` (persisted in `localStorage["flow.theme"]`, safe if storage is blocked).
- Always use tokens (`var(--label)`, `var(--bg-secondary)`…), never raw hex, so both themes work.
- Reduced motion: `--dur*` collapse to ~0 under `prefers-reduced-motion`; use `var(--dur)` for transitions.

### Token reference (names)

| Group       | Tokens                                                                                                                                                                             |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Font        | `--font`, `--font-mono`                                                                                                                                                            |
| Type        | `--fs-large-title 26`, `--fs-title 22`, `--fs-title3 15`, `--fs-headline 13`, `--fs-body 13`, `--fs-callout 12`, `--fs-footnote 11`; `--lh-*`; `--fw-regular/medium/semibold/bold` |
| Spacing     | `--s-1 4`, `--s-2 8`, `--s-3 12`, `--s-4 16`, `--s-5 20`, `--s-6 24`, `--s-8 32`, `--s-10 40`, `--s-12 48`                                                                         |
| Radii       | `--r-xs 4`, `--r-control 6`, `--r-group 10`, `--r-card 12`, `--r-sheet 14`, `--r-pill`                                                                                             |
| Text colour | `--label`, `--label-secondary`, `--label-tertiary` (decorative only), `--label-on-accent`                                                                                          |
| Surfaces    | `--bg-window`, `--bg-secondary` (grouped rows/cards), `--bg-tertiary` (wells, fills), `--bg-hover`, `--bg-pressed`, `--separator`, `--separator-opaque`                            |
| Materials   | `--material-sidebar`, `--material-popover`, `--material-hud` (use with `backdrop-filter: blur(30px) saturate(180%)`)                                                               |
| Accent      | `--accent` (fills, white text), `--accent-hover`, `--accent-pressed`, `--accent-text` (links/text), `--accent-soft`, `--focus-ring`                                                |
| Status      | `--green/-fill/-soft`, `--orange/-fill/-soft`, `--red/-fill/-soft`, `--gray-fill` (`*` = text colour, `*-fill` = dots/icons, `*-soft` = tinted backgrounds)                        |
| Elevation   | `--shadow-1` (rows/cards), `--shadow-2` (popovers/toasts), `--shadow-3` (sheets/HUD), `--keycap-shadow`, `--scrim`                                                                 |
| Motion      | `--ease`, `--dur-fast 120ms`, `--dur 220ms`, `--dur-slow 360ms`                                                                                                                    |

Type utility classes: `t-large-title t-title t-title3 t-headline t-body t-callout t-footnote t-secondary`, plus `.sr-only` and `.selectable`.

## Component catalogue

Every React component below has a plain-CSS equivalent (classes in parentheses). Icons come from `lucide-react` (use `size={16} strokeWidth={1.6}`; thin strokes approximate SF Symbols).

### Button — `.fl-btn`

Props: `variant` `'bordered'|'prominent'|'plain'|'destructive'|'destructive-prominent'`, `size` `'small'|'regular'|'large'`, `icon`, `loading`, `fullWidth`, `tone="destructive"`, plus native button props. A button with no children is an icon button (give it `aria-label`).

```tsx
<Button variant="prominent" icon={<Plus size={14} />}>
  Add Computer…
</Button>
```

HTML: `<button class="fl-btn fl-btn--prominent fl-btn--large">Continue</button>`; modifiers `--prominent --plain --destructive --destructive-prominent --small --large --full --icon`.

### Toggle — `.fl-toggle`

Props: `checked`, `onChange(next)`, `label` (accessible name), `disabled`. Renders `button[role=switch]`.

```tsx
<Toggle label="Open at login" checked={on} onChange={setOn} />
```

### SegmentedControl — `.fl-segmented` > `.fl-segmented__item`

Props: `value`, `onChange`, `options: {value,label,icon?}[]`, `label`. Radio-group semantics, arrow keys, roving tabindex.

```tsx
<SegmentedControl<"a" | "b">
  label="View"
  value={v}
  onChange={setV}
  options={[
    { value: "a", label: "A" },
    { value: "b", label: "B" },
  ]}
/>
```

### Select (pop-up button) — `.fl-select` > `select` + `.fl-select__chev`

Props: `value`, `options: {value,label}[]`, `onChange(value)`, `label`. Native `<select>` underneath.

### Slider — `.fl-slider`

Props: `value`, `min`, `max`, `step`, `onChange(n)`, `label`, `disabled`, `valueText`. Set CSS var `--fill: NN%` for the filled track.

### Section / Row (inset grouped list) — `.fl-section`, `.fl-list`, `.fl-row-wrap`, `.fl-row`

`Section` props: `title?`, `footer?`, `id?`, children = `Row`s. `Row` props: `icon?`, `iconTone?` (`neutral|accent|green|orange|red|gray`), `title`, `subtitle?`, `trailing?`, `onClick?` (whole row becomes a button), `chevron?`, children (extra content below the row, e.g. an inline notice).

```tsx
<Section title="Computers" footer="Explain consequences here.">
  <Row
    icon={<Laptop size={16} />}
    iconTone="accent"
    title="MacBook Pro"
    subtitle="macOS · This computer"
    trailing={<Badge tone="accent">Active</Badge>}
  />
</Section>
```

HTML: `ul.fl-list > li.fl-row-wrap > .fl-row > (.fl-row__icon + .fl-row__main > .fl-row__title/.fl-row__subtitle) + .fl-row__trailing`.

### Sidebar / SidebarItem — `.fl-sidebar`, `.fl-sidebar__nav`, `.fl-sidebar__item`

`Sidebar` props: `footer?`, `label?`. `SidebarItem`: `icon`, `label`, `active`, `onClick`, `badge?`, `shortcut?`. Sidebar collapses to icons below 720 px.

### Toolbar — `.fl-toolbar`

Props: `title`, `subtitle?`, `trailing?`. Renders the page `<h1 id="page-title">`. `--col-pad` (optional, defined by your layout) aligns it with content.

### Badge / StatusDot — `.fl-badge`, `.fl-dot`

`Badge tone` `neutral|accent|green|orange|red`; `StatusDot tone` `gray|green|orange|red|accent`, `pulse`. A dot is decorative: always pair with text.

### Spinner — `.fl-spinner`

Props: `size`, `label` (sets `role=status`). Use `loading` on Button for in-button progress.

### EmptyState — `.fl-empty`

Props: `art?`, `title`, children (one sentence), `actions?`. Use inside `.fl-card` or a section area.

### Sheet — `.fl-scrim`, `.fl-sheet` (`__head __body __foot`)

Props: `open`, `title`, `onClose?`, `footer?`, `width?`, children. Modal: focus trap, Esc closes, focus restored. Put the primary action last in the footer; mark the initial focus with `data-autofocus`.

### Alert — `.fl-scrim.fl-scrim--center`, `.fl-alert`

Props: `open`, `title`, `message`, `actions: {label, variant?, onClick, autoFocus?}[]`, `onClose`. Use for confirmations; the safe action (Cancel) gets `autoFocus`; the destructive action uses `variant: 'destructive-prominent'`.

### Toast — `.fl-toasts`, `.fl-toast`

`<ToastProvider>` once at the root, then `const toast = useToast(); toast.show('Saved'); toast.show('Could not save', 'error')`.

### Hud — `.fl-hud`

Props: `icon`, `title`, `subtitle?`, `leaving?`, `inline?`. Centred macOS-style HUD for transient state changes ("Now controlling Office PC"); non-interactive, auto-dismiss ~1.6 s (you own the timer).

### KeyCap / KeyCombo — `.fl-keycombo`, `.fl-keycap` (`--lg`, `--glyph`)

`KeyCombo` props: `keys: string[]` (canonical tokens: `Ctrl Alt Shift Meta ScrollLock F13 Space A …`), `os`, `large?`. Draws ⌘⌥⌃⇧ on macOS and `Ctrl/Alt/Shift/Win|Super` elsewhere; exposes a spoken label. Helpers: `displayKey`, `spokenKeys`, `labelForKeys`, `tokenFromCode(event.code)`, `heldModifiers(event)`, `isModifierToken`.

### ShortcutRecorder — `.fl-recorder`

Props: `value: string[]|null`, `os`, `onChange({keys,label}|null)`, `label`, `mode` `'chord'|'key'`, `allowClear`, `emptyText`. Click, then press the keys; Esc cancels; emits canonical tokens, never glyphs.

### Tooltip — `.fl-tip`, `.fl-tip__bubble`

Props: `label`, `align?` (`'center'` default, `'end'` for controls at a row's trailing edge so the bubble stays in the window), child element (gets `aria-describedby`). For icon-only buttons.

### Banner — `.fl-banner` (`--orange --red --gray`)

Props: `tone`, `icon?`, `title`, children (one short sentence), `actions?`, `role` (`status`|`alert`). Wrap several in `.fl-banner-region`.

### CodeBlock — `.fl-codeblock`

Props: `children: string` (the command), `onCopy?()`. Monospace command on a tertiary surface with a Copy button that confirms "Copied". Use it for any shell command a person must run; never inline commands in secondary text.

### Layout helpers

`.fl-card` (white rounded panel), `.fl-grid` (gap grid), `.fl-notice` / `.fl-notice--red` (inline warning under a row), `.fl-code` (large mono verification code), `.fl-center` (centred stack), `.fl-glyph-circle` (+ `--green --red --accent`), `.fl-pre` (copyable command), `.fl-field` / `.fl-field__label` (label above a control).

### Hooks / providers

`ThemeProvider`, `useTheme`, `ToastProvider`, `useToast`, `useFocusTrap(open, onClose)` (modal focus management), `cx(...classes)`.

## Gallery

`#/design` in the Flow app (or render `DesignGallery` from `@flow/design-system/gallery`) shows every component in light/dark with a theme switcher.
Screenshots live in `docs/progress/track-c/03*-design-system-*.png`.
