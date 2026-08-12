# Tray quick-add panel — design

Date: 2026-08-11  
Status: approved (brainstorm with owner)  
Repo: `oikonomia`

## Problem

Oikonomia already lives in the system tray (close main window = hide; tray
menu Open / Quit). Capturing a receipt or a one-off expense still requires
opening the full app and navigating Transactions. The owner wants a **small
always-available Spotlight companion** on tray left-click: tray-anchored opaque
card, amount-first manual entry, drag-drop for documents with a brief review
expansion, without losing Open full app and Quit.

## Goals

- Left-click tray → show a **dedicated small window** for fast posting.
- Right-click tray → native menu: **Open Oikonomia**, **Quit Oikonomia**.
- Support all simple kinds: expense, income, bill, transfer.
- Manual entry stays compact (Spotlight idle: kind segment, large amount, soft
  account/memo rows).
- File drop expands the panel for analyzer review, then posts.
- Match existing Stripe-dark (and light) tokens; no new brand language.
- All ledger and OCR rules stay in **`oikonomia-core`** / existing Tauri
  commands; UI only maps form state to those APIs.
- After successful post: brief success state, then hide the panel.

## Non-goals (v1)

- Inline master-password unlock in the tray panel.
- Full journal editor, recurring entries, multi-currency, budgets.
- Settings UI for a fixed “tray default entity” (last-used is enough).
- Global keyboard hotkey (can follow later).
- Resizing or repurposing the main window as the popover.
- New posting/OCR domain logic.

## Confirmed product decisions

| Topic | Decision |
|-------|----------|
| Architecture | Dedicated second Tauri webview (`quick-add`), shared frontend build |
| Left-click | Show / focus quick-add |
| Right-click | Menu: Open Oikonomia, Quit Oikonomia |
| Entry kinds | Expense, income, bill, transfer |
| Vault locked | Panel opens with short message + **Open Oikonomia**; no password field |
| Document drop | Panel expands for review / edit / confirm |
| Entity | Last-used entity; tiny switcher only when multiple entities exist |
| After save | ~1s “Saved”, then hide window and reset form |
| Bill default | Unpaid by default; small unpaid/paid toggle when kind = Bill |
| Memo | Optional; quiet on Save step only |
| Window sizes | Locked: stepper **600×80**, save **600×120**, locked / success / no-books **600×72** (width always 600) |
| Chrome | Opaque soft card (no backdrop-blur); tray-anchored; one-row horizontal stepper |
| On vault lock while open | Clear draft; switch to locked UI immediately |

## Architecture

```
┌──────────────── tray icon ────────────────┐
│ left-click  → show/focus window "quick-add"│
│ right-click → menu Open / Quit             │
└────────────────────┬──────────────────────┘
                     │
     ┌───────────────┴────────────────┐
     ▼                                ▼
 window "main"                  window "quick-add"
 full SPA (existing)            QuickAddApp only
     │                                │
     └──────── invoke (shared) ───────┘
                     │
                     ▼
            oikonomia-core + existing commands
            entry_post_simple
            document_analyze / _path
            entry_post_simple_with_document / _path
            vault_status, entity_list, account_list
```

### Shell (Rust / Tauri)

- Extend `apps/desktop/src-tauri/src/tray.rs` (and window helpers as needed):
  - `show_menu_on_left_click(false)`.
  - Tray icon event: primary click → `show_quick_add_window`.
  - Menu: `open` → existing `show_main_window`; `quit` → `app.exit(0)`.
- Window label: **`quick-add`**.
  - Create on first need if missing; thereafter **show / hide / focus** (reuse).
  - Locked sizes (logical px; stay in lockstep with `QUICK_ADD_*` in
    `apps/desktop/src-tauri/src/tray.rs` and `web/src/lib/quickAddWindow.ts`):
    - Width **600**
    - Stepper (default unlocked rolls): **600×80**
    - Save / confirm: **600×120**
    - Locked / success / no-books (compact): **600×72**
  - Not bound by main’s `minWidth` 960 / `minHeight` 640.
  - Always-on-top while visible; resizable false; undecorated transparent native
    shell so the webview can draw an **opaque** soft Spotlight card (rounded
    corners; no backdrop-blur in v1).
  - Position **tray-anchored** (near tray click); FE never recenters. Fallback:
    stable corner of the primary work area when bounds are unavailable.
  - CloseRequested: prevent destroy if reusing; **hide** instead (same spirit
    as main → tray). Escape from the webview also hides.
- Main window, Dock reopen, window-state plugin behavior: **unchanged**.
- Optional thin command `quick_add_hide` if frontend should not depend on
  window API permissions alone; prefer existing Tauri window APIs if the
  capability already allows it.

### Frontend mount

- Same Vite build and `index.html` for both windows.
- `web/src/main.tsx` branches on the current window label
  (`getCurrentWindow().label === 'quick-add'` → mount `QuickAddApp`;
  else mount existing `App`). No full app router required for v1.
- New module(s), e.g. `web/src/pages/QuickAddPage.tsx` (or
  `web/src/quick-add/…`), reusing `components/ui`, `lib/api`, `lib/cn`,
  document helpers, and the same kind → field rules as Transactions
  (defaults / account role filters). Extract shared pure helpers from
  `TransactionsPage` only where it avoids duplication without a large
  refactor.

### Business logic

- **No new core posting paths.** Quick-add builds `SimpleEntryInput` /
  `PostSimpleEntry` exactly as the full form does.
- Document flow reuses analyze + post-with-document commands and the same
  size / MIME gates as `DocumentDropZone`.

## UI states

### Unlocked, stepper (default) — 600×80

One-row horizontal stepper (opaque soft card). Current step exits left; next
enters from the right. Back `‹` from step 2+. Esc hides. Date is `todayISO`
only — no date control.

1. **Book** — pick book (skipped when a single entity exists).
2. **Type** — Out / In / Bill / Move → expense / income / bill / transfer;
   selecting advances.
3. **Amount** — 22–24px tabular input, autofocus; Enter advances.
4. **Accounts** — role pickers on one row for the kind.

Account roles per kind (same as full simple form / Rust):

| Kind | Fields |
|------|--------|
| Expense | category (expense), wallet (asset/liability pay-from) |
| Income | category (income), wallet (deposit-to) |
| Bill | category (expense), payable (AP) or wallet when paid |
| Transfer | from account, to account |

### Save / confirm — 600×120

Quiet extras live only here: Bill Due/Paid, optional memo, Drop receipt,
accent **Save**, **Cancel** (hide + reset). Analyze shows one-row
**Analyzing…** at stepper height, then Save prefilled (document-aware post).

### Locked — 600×72

Copy: vault is locked. Primary action: **Open Oikonomia** (show/focus main).
No form fields. Same pattern if there are zero entities: prompt to create a
book in the full app + Open. Compact height.

### Success — 600×72

~1 second “Saved” (kind + amount) at compact height, then hide window and reset
form state for the next open.

### Validation / errors

Inline errors on the panel; do not hide on failed post. Oversized or
unsupported files use the same messages/limits as the main drop zone.

## Preferences

Extend non-secret `UiPrefs` in `crates/oikonomia-core/src/prefs.rs`
(plaintext `ui-prefs.json`, already used for theme):

- `last_entity_id: Option<String>` (or typed id string as elsewhere in IPC).
- `last_accounts_by_entity_kind`: map keyed by entity + kind → role account
  ids used last successful **tray** post (category, wallet, payable, from, to
  as applicable).

Notes:

- `UiPrefs` is currently `Copy` with only `theme`. Adding strings/maps means
  dropping `Copy` (keep `Clone`); update prefs tests accordingly. Unknown
  fields remain tolerated via `#[serde(default)]`.
- **v1:** update these prefs on successful tray post only. Main app entity
  selection need not write them unless it is trivial later.
- On open: resolve entity = last_entity if still present, else first entity;
  accounts = last map entry for entity+kind if still active, else same
  kind-defaults as Transactions.

Expose get/set via existing settings-style commands or a dedicated
`settings_get/set_quick_add_prefs` pair — keep plaintext prefs out of the
encrypted vault.

## Events and security

- Listen for existing `vault-locked` in QuickAddApp: set locked UI, clear
  draft (including pending document bytes/path). Do not leave a postable
  form after lock.
- Idle auto-lock remains the Rust watchdog (`spawn_auto_lock`); tray UI does
  not own lock policy.
- v1 capabilities: still **no network**. Document path post must keep the
  same path validation as main (no arbitrary filesystem expansion of trust).
- Activity in the quick-add window should touch the same activity heartbeat
  path the main window uses (so typing in tray does not immediately idle-lock
  if the user is actively posting). Reuse or mirror main’s throttled
  `vaultStatus` / activity touch pattern.

## Edge cases

| Case | Behavior |
|------|----------|
| Main already visible | Quick-add is independent; posts succeed either way |
| Main hidden to tray | Unchanged; quick-add does not force main open except Open action |
| Auto-lock mid-edit | Draft cleared; locked UI |
| Analyzer failure | Error in expanded panel; user can dismiss or retry |
| Invalid accounts / zero amount | Validation error; panel stays open |
| Multi-monitor / no tray rect | Fallback position (cursor or primary work area) |
| Theme | Respect stored theme (same `getTheme` / document class as main) |

## Testing

- **Core prefs:** load/save round-trip for new fields; corrupt/missing file
  still yields defaults; unknown fields tolerated.
- **Core ledger/OCR:** no intentional behavior change; existing
  `simple_entry` and documents tests remain the gate.
- **Frontend:** pure mapping tests for kind → `SimpleEntryInput`; locked vs
  unlocked vs success render smoke if lightweight tests exist; otherwise
  build + lint.
- **Manual / desktop:** left-click opens panel; right-click menu Open/Quit;
  post expense/income/bill/transfer; drop PDF → expand → confirm; Escape /
  blur hide; lock while open; multi-entity switcher; success then reopen
  shows clean form with last-used defaults.

## Implementation sketch (for the plan, not binding order)

1. Prefs fields + tests; thin IPC if needed.
2. Tray event split (left vs menu); create/show/hide `quick-add` window;
   capabilities for the new window if required.
3. Frontend mount branch + QuickAddApp shell (locked / unlocked / success).
4. Idle form + post via `entry_post_simple`; last-used defaults.
5. Drop + expand review + document post path.
6. Polish: position, focus, theme, activity heartbeat, Escape.

## Open implementation details (resolve in plan, not product)

- **Resolved — window sizes:** locked at stepper **600×80**, save
  **600×120**, locked / success / no-books **600×72** (width **600**).
  Constants must stay in lockstep between `tray.rs` `QUICK_ADD_*` and web
  `QUICK_ADD_*` (`quickAddWindow.ts`). One-row rolling stepper; quiet extras
  only on Save.
- Whether blur-to-dismiss is enabled on all platforms or only when not
  interacting with a native file dialog.
- Whether `pay_existing` bill flow appears in tray (product: unpaid/paid
  toggle only; **pay existing bill is full-app only**).
