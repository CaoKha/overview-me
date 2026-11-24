# Design system

The overview page (`overview-me page`) takes every color from the tokens in
`assets/styles/tokens.css`. A palette swaps those tokens, so the whole page,
code highlighting and mermaid diagrams included, follows it.

## Rules

- No raw colors outside `assets/styles/tokens.css` and `assets/themes/`. Use `var(--token)`.
- Each color token is `light-dark(<light>, <dark>)`. The system picks the mode
  unless the host sets `data-theme="light"` or `data-theme="dark"` on `<html>`.

## Color tokens

| Token | Use |
|---|---|
| `--bg` | Page background |
| `--card` | Cards, tables, panels, code |
| `--popover` | Detail panel, inline code |
| `--border` | Borders and dividers |
| `--fg` | Body text |
| `--accent` | Secondary text, icons |
| `--muted` | Hints, comments, line numbers |
| `--edge` | Flow diagram edges, resize handles |
| `--hl` | Highlight: focus rings, active controls, highlighted edges |
| `--on-hl` | Text and icons on `--hl` |
| `--ok`, `--ok-bg` | Added, passing; strings in code |
| `--warn`, `--warn-bg` | Warnings, located flow boxes; titles in code |
| `--bad`, `--bad-bg` | Removed, failing, high-severity findings |
| `--id`, `--id-bg` | Node ids, note chips; numbers in code |
| `--link` | Links; keywords in code |
| `--secondary` | Button fill, hovered and current tree rows |

## Palettes

A palette is one rule that overrides any of the color tokens:

```css
:root[data-palette="ocean"] {
  --bg: light-dark(#F2F7FA, #0B1620);
  --card: light-dark(#FFFFFF, #10202D);
  --hl: light-dark(#0A6FA8, #5FC3F0);
}
```

Tokens it leaves out keep their Default value.

- Built-in palettes live in `assets/themes/` and are listed in `STYLES` in `src/page.rs`:
  `catppuccin` (Latte in light mode, Mocha in dark mode).
- Add your own with `overview-me page <file.md> --theme ocean.css` (repeatable).
  The file is inlined into the page. A file holding `</style` is refused.
- The picker in the page header lists Default plus every `data-palette` name in
  the page's styles. It saves the reader's choice in `localStorage`
  (`overview-me:palette`).
