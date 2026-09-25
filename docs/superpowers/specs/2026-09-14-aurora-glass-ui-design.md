# Aurora glass UI — design

Date: 2026-09-14
Status: approved 2026-09-14
Scope: the visual redesign of the Oikonomia desktop UI (main window, dialogs,
unlock screen, tray quick add) and the Rust work it depends on.
Visual reference (approved): https://claude.ai/code/artifact/4a5a5904-43b0-4d9e-9a4a-4a5b48c28938
— Dashboard, Transactions, New entry, Controls, Chart colours.

## Context

Round 1 (PR #59, closed, branch deleted) ported the GDA plugins' Livery
*chassis* — tokens, square corners, mono captions, hairlines — onto the
composition the app already had. It was rejected as "very much the same as
before". Two causes, both avoided here:

1. It reskinned the existing layout (hero, four metric cards, activity list)
   instead of changing the composition.
2. It took the plugins' cold chassis and skipped their light: signal drawn as
   fluid, glowing colour, which is what actually makes the faceplates striking.

Round 2 mocked three directions as real 1280 x 800 windows before any code.
Direction C, Aurora glass, was chosen, then carried to four screens and
refined with the user.

## Decisions (locked)

1. **Direction C, Aurora glass.** A slowly drifting aurora fills the window
   behind rounded frosted-glass panes. Money is drawn as light.
2. **No particles.** No sparkles inside the light, no grain texture behind the
   aurora.
3. **Ledger chart colours.** Money in `#1BA39A` (teal-green), money out
   `#E8603F` (coral). Checked with the dataviz palette validator against the
   pane surface `#11141B`: every check passes; colour-blind separation
   ΔE 13.8 (deutan), normal-vision ΔE 27.6. Classic green/red and
   cyan/violet were tried and failed (ΔE 4.3 and 4.1).
4. **Two colour roles, never mixed.**
   - *Brand light* `#2EE6A6 -> #37D5FF -> #7A8CFF`: chrome only — primary
     buttons, active navigation, logo, quick add, focus.
   - *Ledger*: anything that is money — the cash-flow light, arcs, amount
     pills, the net figure, the Expense/Income type in dialogs.
5. **Dark-only.** Supersedes "dark/light" in
   `2026-08-10-oikonomia-design.md`. The theme preference is retired end to
   end: `settings_get_theme`, `settings_set_theme`, `native_theme` and
   `UiPrefs::theme` are removed, and startup sets `tauri::Theme::Dark`
   unconditionally. `UiPrefs` is `#[serde(default)]` and ignores unknown keys,
   so prefs files written by older builds still load.
6. **Business logic stays in Rust** (repo invariant). Every figure the light
   and the arcs draw is computed in `oikonomia-core`; the UI maps numbers to
   pixels and nothing else.
7. **Typography, with Greek covered.** Barlow 400/500/600 for UI text, IBM Plex
   Mono 500 for captions, Sofia Sans 900 for display (watermark, logo). Barlow
   and IBM Plex Mono have **no Greek glyphs** (verified against the Google
   Fonts subset list), and the app ships an `el` locale, so the stacks fall
   back per glyph: `"Barlow", "Sofia Sans", ...` and
   `"IBM Plex Mono", "JetBrains Mono", ...` — Sofia Sans and JetBrains Mono
   both carry Greek. All faces are bundled as woff2 under `web/public/fonts`
   with their SIL OFL texts; nothing is fetched at runtime.
8. **The PDF export is unchanged.** It keeps its own palette and Inter, a
   document font chosen for Greek coverage that pdf-lib can embed.

## Visual system

### Surfaces

| Token | Value | Use |
|---|---|---|
| ground | `#05060A` | window, behind the aurora |
| aurora | five radial blobs: emerald, cyan, indigo, magenta, amber; `blur(70px) saturate(1.35)` | ambient colour |
| pane | `rgba(16,18,24,0.50)`, `backdrop-filter: blur(28px) saturate(170%)`, 1px `rgba(255,255,255,0.09)` edge, inset top highlight | every panel |
| veil | a top-down darkening over the aurora, starting at 0.5 opacity | keeps text that sits directly on the aurora legible |
| scrim | `rgba(4,5,8,0.42)` + `blur(10px)` | behind dialogs |
| watermark | "ΟΙΚΟΝΟΜΙΑ", Sofia Sans 900, 232px, 1px outline at 7% white | decorative, behind panes |

Radii: panes 22px, tiles 20px, controls 12px, pills and segmented controls
fully rounded.

### Ink and signals

| Token | Value | Use |
|---|---|---|
| ink | `#E7EBF1` | primary text, values |
| ink-mid | `#AEB5BF` | secondary text |
| ink-soft | `#A3AAB5` | hints, captions, placeholders |
| ink-dim | `#848B98` | icons and inactive controls only, never text |
| money-in / text | `#1BA39A` / `#6FD9C4` | Ledger in; text tint for legibility |
| money-out / text | `#E8603F` / `#FF9B7E` | Ledger out |
| hazard | `#E2F23A` | attention: unpaid bill, warnings |
| danger / danger-text | `#FF8295` / `#FFD3DA`; `#D7304C -> #B8327A` destructive button fill (white text) | `#FF8295` is text and icons directly on glass; `#FFD3DA` (`--color-danger-text`) is text on the danger-soft plate (error banners) — the plate is not bare glass, so it needs its own lighter tint to hold 4.5:1 |
| success | the brand light's emerald, `#2EE6A6` | confirmations only (a saved template, a completed action) — **never** money; money identity (amounts, icons, badges) always uses the Ledger, so a success confirmation and money coming in are never drawn in the same colour |

Status colours always ship with an icon and a label, so coral money-out is
never read as an error.

**Measured, not assumed (2026-09-14).** Text was checked against the
99th-percentile brightest backdrop pixel under any pane, across five points
in the aurora's drift. At the mocked pane fill of 0.42 the old secondary greys
failed (ink-mid 4.0:1, ink-soft 3.0:1), so the fill rises to 0.50 and the greys
lighten. Worst case at 0.50: ink 9.6, ink-mid 5.6, ink-soft 4.9, money-in text
6.8, money-out text 5.6; ink-dim is 3.3:1, which is why it never carries text.
The mocked destructive button (white on `#FF4D67`, 3.2:1) also failed, hence
the deeper fill. Text directly on the aurora gets no help from a pane; there
the veil must be at least 0.38 at the brightest point, and phase 1 measures it
on the running app before merging.

Gradient stops for the light: in `#27BF93 -> #1BA39A -> #1E8DB0`, out
`#F07A45 -> #E8603F -> #D64A5A`.

The Reports `--viz-*` categorical palette is unchanged, but is re-validated
against the pane surface `#11141B` during implementation.

### Components

The existing primitives in `web/src/components/ui.tsx` keep their names and
props; their treatment changes. Page code mostly does not.

| Primitive | Aurora glass treatment |
|---|---|
| `Card`, `Panel`, `CollapsibleSection` | glass pane |
| `Hero` | glass pane hosting the cash-flow light |
| `MetricCard` | arc tile: 270° arc with bloom and a lit pointer dot, value in ink |
| `Button` | primary: brand gradient, dark text; secondary: glass; danger: danger gradient; ghost unchanged |
| `Input`, `Select`, `Field` | label inside the field above the value; focus: cyan ring and glow; error: danger ring plus message below |
| `Segmented` | glass, fully rounded, active option on a lifted plate |
| `FlowBar` | replaced on the dashboard by the light; elsewhere drawn in Ledger colours |
| `ListRow` | hover surface; row actions revealed on hover **and** on `:focus-within` |
| `EmptyState`, `ErrorBanner`, `IconBadge`, `ChoiceCard` | glass and tinted variants as in the Controls mockup |
| `Modal`, `ConfirmDialog` and every modal | glass dialog over the blurred scrim |

New components:

- `Aurora` — the ambient layer, mounted once under the app root. It holds
  still while the vault is locked, until phase 4 makes the unlock screen a
  glass card over it.
- `CashFlowLight` — the light (see Rendering).
- `Arc` — the 270° meter.

## Screens

- **Shell.** A floating glass sidebar (220px): logo, navigation, the list of
  books (replacing the book dropdown), and a Quick add button at the bottom.
  The top bar carries the page title, the period control and Lock.
- **Dashboard.** New composition: a hero pane with the net figure, in and out
  pills and the cash-flow light across its lower half; a row of four arc
  tiles; a recent-activity list.
- **Transactions.** A period summary pane with a strip of the light beside a
  compact drop zone; one glass filter row (search, from, to, account); the
  entries list with Recurring, Import CSV and Export CSV in its header. The
  light strip follows the date filter only and stays entity-wide; when an
  account filter is set, its caption says so.
- **Dialogs.** Glass over the blurred page. The chosen entry type colours the
  type selector (Expense and Bill in the out gradient, Income in the in
  gradient, Transfer neutral). Dialogs render through a portal to
  `document.body`: `backdrop-filter` makes an element the containing block
  for `position: fixed` descendants in WebKit, so a dialog opened inside
  another dialog (or any other glass surface) would otherwise be sized to
  and clipped by it instead of the viewport.
- **Unlock.** A glass card over the aurora.
- **Quick add (tray).** The transparent tray window becomes glass. CSS
  `backdrop-filter` cannot blur the desktop behind a transparent OS window, so
  real blur there needs native window effects (Tauri window effects on macOS,
  `macOSPrivateApi` is already enabled). The exact API is verified against
  Tauri 2 docs in the plan before use.
- **Settings, Accounts, Reports, Documents, Recurring.** Restyled through the
  primitives; no layout changes.

## Data and Rust

### `cash_flow_series`

`oikonomia-core` gains:

```rust
pub fn cash_flow_series(
    conn: &Connection,
    entity_id: EntityId,
    from: &str,
    to: &str,
) -> Result<CashFlowSeries>
```

- **Buckets:** one per day when the window is 92 days or shorter (covers a
  month and a quarter), otherwise one per calendar month (a year shows 12).
  The bucketing rule lives in Rust.
- **Each bucket:** start date, `income_minor`, `expenses_minor`, and the
  running `cumulative_income_minor` and `cumulative_expenses_minor`, all `i64`
  minor units.
- **Same basis as `dashboard_summary`:** income is credits minus debits on
  Income accounts; expenses are debits minus credits on Expense accounts; the
  entry filter is `ACTIVE_ENTRY_PREDICATE` (voided entries and their reversals
  excluded), placed in an inner-join subquery per the repo invariant — never
  in a `LEFT JOIN ... ON` clause.
- **IPC:** `cash_flow_series_cmd(entity_id, from, to)` in `commands.rs`, plus
  a typed wrapper in `web/src/lib/api.ts`.

Invariants, each a test:

1. The final cumulative income and expenses equal `dashboard_summary`'s
   `income` and `expenses` for the same window.
2. Buckets are contiguous and cover `from..=to` exactly; an entry dated on a
   bucket boundary lands in exactly one bucket.
3. An empty window returns zero-valued buckets, not an error; `from > to` is a
   validation error, as in `dashboard_summary`.
4. Voided entries and void reversals contribute nothing.

### Arc metrics

`DashboardSummary` gains computed, optional fields in basis points, so the UI
never divides:

- `savings_rate_bps` — net / income; `None` when income is zero or less.
- `spend_ratio_bps` — expenses / income; `None` when income is zero or less.
- `top_expense` — the largest Expense account in the window and its share of
  expenses; `None` when there are no expenses.
- `net_vs_previous_bps` — net for the window versus net for the equally long
  window immediately before it; `None` when the previous net is zero.

A `None` renders as an empty arc with an em dash, never as 0%.

## Rendering

- **Aurora.** CSS radial gradients under a single blur filter, animated with
  `transform` only (22–30 s cycles) so it stays on the compositor. It pauses
  when the document is hidden or the window loses focus, and does not animate
  at all under Reduce Motion.
- **CashFlowLight.** Two stacked canvases: a blurred glow layer (fills only)
  and a sharp layer (gradient fill at low alpha, a soft vertical sheen, the
  zero line, and bright ridges on both edges). In rises above the zero line
  and out hangs below it, on one shared scale. The geometry — series to
  points — is a pure module, `web/src/lib/cashFlowLight.ts`; the component
  only paints. It redraws at most around 30 fps, only while visible, only
  while motion is allowed; otherwise it paints once. The canvas carries a text
  alternative stating the period's in, out and net.
- **Arc.** SVG: a track, a gradient stroke with a blurred bloom copy, and a
  pointer dot. Exposed as a meter with its value and label.
- **Glass performance.** `backdrop-filter` over an animated layer is the most
  expensive part of the design. Glass over the drifting aurora ships to every
  page in phase 1 (foundation), not only the dashboard, so its measurement
  belongs to phase 1's running-app review, not "before the dashboard phase
  merges": on the bundled `.app` (never the bare binary, which renders
  blank), unlocked Dashboard idle and while scrolling a long Transactions
  list, including the `com.apple.WebKit.GPU` process. Where WebKit supports
  `prefers-reduced-transparency`, panes fall back to solid.

## Accessibility

- In and out differ by position (above or below the line), sign and label —
  never by colour alone.
- Body text at least 4.5:1 and UI elements at least 3:1, measured against the
  pane in its brightest aurora position; pane opacity rises where needed.
- Row actions appear on keyboard focus, not only on hover.
- Visible focus ring on every interactive element.
- Reduce Motion stills the aurora and the light.

## Testing

- **Rust.** Unit tests for the four `cash_flow_series` invariants and for each
  arc metric's `None` cases. Extend the randomised ledgers in
  `tests/report_correctness.rs` so that for random ledgers the series' final
  cumulative totals always equal `dashboard_summary`. A prefs test that a file
  containing the retired `"theme"` key still loads.
- **Web.** Vitest for the pure light geometry and the arc value-to-angle
  mapping, and for the new API wrappers. The existing 304 tests keep passing;
  where they select by role and accessible name that holds by construction,
  and any test coupled to class names or markup changes with its component.
- **Visual QA.** The Playwright harness with a stubbed Tauri IPC layer
  captures every screen at 1280 x 800, in English and in Greek, and each is
  compared against the approved mockup.
- **Smoke.** `scripts/smoke-macos.sh` render check on the `.app`, plus the
  idle and scroll measurements above.

## Rollout

Each phase is its own PR, green on CI. The user reviews the **running app**
after phases 1 and 3 — round 1's lesson was to show it early.

1. **Foundation.** Fonts with Greek fallback, tokens, `Aurora`, glass
   primitives, dark-only, theme retirement in Rust. Every page picks up the new
   look through the primitives.
2. **Data.** `cash_flow_series`, arc metrics, IPC commands and TypeScript
   wrappers, with their tests.
3. **Composition.** Dashboard, Transactions and all dialogs, with
   `CashFlowLight` and `Arc`.
4. **Edges.** Unlock screen, the Quick add tray window, remaining pages, and
   the Greek-locale pass.

## Out of scope

- A light theme.
- The PDF export's appearance.
- The Reports charts' palette, beyond re-validation.
- New reports or bookkeeping behaviour.

## Resolved at review (2026-09-14)

The user approved every recommendation below.

1. **Motion.** The aurora drifts and the light breathes gently; both pause
   when the window is hidden or loses focus, and do not animate under Reduce
   Motion.
2. **The runway tile is replaced by Net vs last period.** There is no honest
   basis for runway yet: `cash_like_assets` sums every Asset account,
   receivables and equipment included, and no account is marked liquid.
   The fourth arc metric is `net_vs_previous_bps`, computed in Rust from the
   window and the equally long window immediately before it. Runway returns
   once accounts can be marked liquid.
3. **Behaviour shown in the mockups that does not exist today:**
   - Quarter in the period control: included.
   - Books list in the sidebar instead of the dropdown: included.
   - Quick add button in the sidebar, opening New entry: included.
   - The `⌘K` shortcut: deferred.
   - Counts beside navigation items: deferred.
   - The "Matched" document chip: dropped; the dialog shows only what
     document analysis actually returns.

## Resolved in the phase 2-3 plan (2026-09-15)

Plan: `docs/superpowers/plans/2026-09-15-aurora-glass-light.md`. These refine the sections above; where they differ, these win.

1. **D1 Previous window.**
   - A window of whole calendar months steps back by the same number of calendar months: September against August, a quarter against the one before, a year against the year before.
   - Any other window steps back by the same number of days.
   - "Equally long" measured in days would have compared September with 2-31 August.
2. **D2 The series carries its totals.** `CashFlowSeries` includes `total_income_minor`, `total_expenses_minor` and `net_minor`, so Transactions shows a net without subtracting in the UI.
3. **D3 Open date filters resolve in Rust.**
   - `activity_window` turns an empty bound into the book's first or last active entry date.
   - With no entries, the open side falls back to the other bound, or to today.
   - `cash_flow_series_cmd` takes `from` and `to` as optional.
4. **D4 The net figure's gradient.**
   - In: `#6FD9C4 -> #27BF93 -> #1BA39A`. Out: `#FF9B7E -> #F07A45 -> #E8603F`.
   - Every stop holds 3:1 on the brightest glass, which is enough because the figure is always large text.
   - The light itself keeps the Rendering stops.
5. **D5 Pill labels use ink-mid** (`#AEB5BF`, 4.54:1 or better on every plate). Ink-soft is 4.04:1 on the money-in plate.
6. **D6 The top bar belongs to the page.**
   - Pages put their title and actions in the header through `TopBar`.
   - Pages that do not keep the book name and chart.
   - Lock becomes a round glass icon button.
7. **D7 Total assets moves into the hero** as a neutral ASSETS pill under IN and OUT.
8. **D8 The dashboard drops:**
   - the explanatory paragraph;
   - the offline-reader caption;
   - the income and expense bars;
   - the "Encrypted vault · local only" line;
   - the Overview eyebrow and the second copy of the book name;
   - the four metric cards.

   The entries count moves into Recent activity's description.
9. **D9 Transactions.**
   - New Entry moves to the top bar.
   - The eyebrow, description and meta line go.
   - The summary follows the date filter only, and says so when account or search filters narrow the list.
10. **D10 The light's zero line moves.** It sits where both peaks fit (30-80% of the plot height), on one shared scale. The mockup's fixed 74% only fit a month where income was well above expenses.
11. **D11 Retired components.** `FlowBar` is deleted, since the light replaces its only use. `MetricCard` stays for Accounts until phase 4.
12. **D12 Quarter bounds are computed in the web**, like month and year: calendar presentation, not accounting.
13. **D13 Quick add** is a full-width primary button in the sidebar, without the `⌘K` hint while the shortcut is deferred.
14. **D14 One PR for phases 2 and 3**, stacked on PR #60, at the user's request. The user reviews the running app after it.
15. **D15 Status tones.**
    - `CollapsibleSection` no longer accepts the success tone; Settings' Books section uses accent.
    - Asset badges keep accent: success and accent share the brand emerald by definition, and a badge is identity, not a confirmation.
16. **Entry type selectors** wear the light's gradient stops with dark labels (`#06110D` on in, `#1A0906` on out), 4.59:1 or better at every stop.
