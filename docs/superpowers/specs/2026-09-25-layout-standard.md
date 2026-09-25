# Layout standard (Aurora glass)

The sizing and alignment rules every screen follows. They come from the 4/8-pt
spacing grid (Material 3, Apple HIG), a constrained type scale, WCAG 2.2 SC 2.5.8
(24x24 CSS px minimum target) and the nested-radius rule. They were set on
2026-09-25 from a measured audit of every view, anchored on the values the code
already used most.

## Spacing

- Layout spacing (pane padding, gaps between panes, row padding, page gutters) is
  on the 4px grid: 4, 8, 12, 16, 20, 24, 28, 32, 40, 48.
- Inside a component under about 40px tall, 2px and 6px steps are fine.
- Sibling panes and cards on a page are 16px apart (`space-y-4`, `gap-4`).

## Type scale (px)

| Size | Role |
|---|---|
| 11 | Mono uppercase labels: eyebrows, field labels, pill labels, table headers. Always `font-mono text-[11px] font-medium tracking-[0.14em] uppercase`. |
| 12 | Captions and meta lines |
| 13 | Descriptions, segmented controls |
| 14 | Body, list-row titles, controls, navigation |
| 16 | Pane and section titles (`text-base leading-6`) |
| 20 | Page titles (the top bar) and dialog titles |
| 24 | Metric figures, auth screen titles |
| 32 | Secondary hero figures |
| 64 | The dashboard hero figure (its upper clamp) |

The same role always gets the same size, weight, colour and tracking. The Quick add
tray (300x64) keeps its own dense 28px control rhythm, but its text uses this scale.

## Radii (px)

8 small swatches and chips, 12 controls, buttons, inputs and icon tiles, 16 inner
cards, 20 panes and cards, 24 the hero pane and dialogs, full for pills. An element
inset N px inside a rounded parent uses the parent radius minus N.

## Controls

- Heights: 32 (`size="sm"`, `size="iconSm"`) and 40 (default, `size="icon"`); 48
  (`size="lg"`) only for the auth screen's primary action.
- Controls in one row share one height. A field box (label inside) is 56px tall;
  buttons beside field boxes are centred on them.
- `cn()` only joins class names. A size passed through `className` does not
  override a variant's own size, so use a `size` value, never `h-8 w-8` on a
  `size="icon"` button.
- Every target is at least 24x24; row actions are 32x32.

## Alignment

- One page header everywhere: each page renders `<TopBar title subtitle actions>`;
  its actions sit in the header's right slot beside the lock button.
- Each pane has one horizontal inset shared by its header, rows, footer and any
  chart inside it: 20px for panes, 28px for the dashboard hero.
- Repeated rows share height, icon tile, text start and right edge. Row actions sit
  in fixed slots (an empty `size-8` span stands in for a missing action), so icons
  form columns.
- Numbers, dates and codes use tabular figures. Codes sit in a fixed-width column
  so names line up. Money is right-aligned.
- Sidebar nav rows and book rows share height, inset and icon column.
- Tiles in a grid top-align their content, so figures share a baseline when a
  label wraps.
