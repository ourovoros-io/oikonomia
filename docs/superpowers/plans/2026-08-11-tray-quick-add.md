# Tray Quick-Add Implementation Plan

> Completed and merged (see git history). Checkboxes below were never ticked during execution; do not re-execute.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Left-click the system tray to open a small always-on-top companion window that posts simple entries (and document drops) without opening the full app; right-click keeps Open / Quit.

**Architecture:** A second Tauri webview (`label: "quick-add"`) reuses the same Vite build. `main.tsx` mounts `QuickAddApp` when the window label is `quick-add`, otherwise the existing `App`. Posting and OCR go through existing `entry_post_simple` / document commands. Last-used entity and role accounts live in plaintext `UiPrefs`. Tray left-click is handled in Rust (`show_menu_on_left_click(false)` + `on_tray_icon_event`).

**Tech Stack:** Tauri 2 (`tray-icon`), `oikonomia-core` prefs, React 19 + Vite + Tailwind, Vitest for pure TS helpers.

Spec: `docs/superpowers/specs/2026-08-11-tray-quick-add-design.md`

## Global Constraints

- Business rules stay in Rust (`post_simple_entry`, document analyze/post). UI only maps form state to existing commands.
- Money is integer minor units on the wire; never `f64` for currency math (parse via existing `parseMajorToMinor`).
- No network capability. No emojis in UI, code, or commits. No `Co-Authored-By`.
- `UiPrefs` is non-secret plaintext in the data dir (same file as theme); never put passwords there.
- Vault lock authority remains `spawn_auto_lock` + `vault-locked` event; tray panel does not implement unlock.
- Bill tray v1: `unpaid` | `paid` only — **no** `pay_existing`.
- Memo line is **always visible** (second micro-row). Idle window ~420×110; document review ~420×320 (exact px adjustable).
- Blur-to-dismiss: hide on `window` blur **unless** `busy` (analyzing/posting). Escape always hides when not busy.
- Gate after Rust tasks: `cargo test -p oikonomia-core` and `cargo clippy -p oikonomia --all-targets -- -D warnings` (from workspace root; desktop crate name is `oikonomia`).
- Gate after frontend tasks: `cd web && npm run test && npm run build && npm run lint`.

## File map

| Path | Responsibility |
|------|----------------|
| `crates/oikonomia-core/src/prefs.rs` | Extend `UiPrefs` with last entity + last accounts map |
| `apps/desktop/src-tauri/src/tray.rs` | Left-click → quick-add; right-click menu Open/Quit; create/show/hide/position window |
| `apps/desktop/src-tauri/src/commands.rs` | Prefs IPC, `quick_add_hide`, `open_main_window` |
| `apps/desktop/src-tauri/src/lib.rs` | Register commands; window-close hide already applies to all windows |
| `apps/desktop/src-tauri/capabilities/default.json` | Allow `quick-add` window + needed window/event permissions |
| `apps/desktop/src-tauri/tauri.conf.json` | Optional second window declaration (plan prefers **runtime create** in tray) |
| `web/src/main.tsx` | Branch mount on window label |
| `web/src/QuickAddApp.tsx` | Shell: theme, vault status, locked/success/unlocked routing |
| `web/src/pages/QuickAddPage.tsx` | Dense form, drop, document review, post |
| `web/src/lib/simpleEntry.ts` | Shared pure helpers: kind defaults, build `SimpleEntryInput`, account filters |
| `web/src/lib/simpleEntry.test.ts` | Unit tests for helpers |
| `web/src/lib/api.ts` | Types + API wrappers for new prefs/window commands |

---

### Task 1: Extend `UiPrefs` for last-used tray defaults

**Files:**
- Modify: `crates/oikonomia-core/src/prefs.rs`
- Test: same file `#[cfg(test)]` module (existing pattern)

**Interfaces:**
- Produces:
  - `LastRoleAccounts { category_account_id, wallet_account_id, payable_account_id, from_account_id, to_account_id }` each `Option<String>`
  - `UiPrefs { theme, last_entity_id: Option<String>, last_accounts_by_entity_kind: BTreeMap<String, LastRoleAccounts> }`
  - Map key format: `"{entity_id}:{kind}"` where kind is one of `expense|income|bill|transfer` (document as constant in docs/comments; helper optional)
  - `UiPrefs` is `Clone` but **not** `Copy` after this change
  - Existing `load_ui_prefs` / `save_ui_prefs` signatures unchanged

- [ ] **Step 1: Write failing tests for new fields**

Add to the `#[cfg(test)]` module in `prefs.rs` (keep existing tests compiling — they construct `UiPrefs { theme: ... }` and must use `..Default::default()` or full fields after the struct change):

```rust
#[test]
fn last_used_round_trips() {
    let Ok(dir) = tempdir() else {
        return;
    };

    let mut last_accounts = BTreeMap::new();
    last_accounts.insert(
        "ent-1:expense".to_string(),
        LastRoleAccounts {
            category_account_id: Some("cat-1".into()),
            wallet_account_id: Some("wal-1".into()),
            payable_account_id: None,
            from_account_id: None,
            to_account_id: None,
        },
    );

    let prefs = UiPrefs {
        theme: Theme::Dark,
        last_entity_id: Some("ent-1".into()),
        last_accounts_by_entity_kind: last_accounts,
    };
    assert!(save_ui_prefs(dir.path(), &prefs).is_ok());
    assert_eq!(load_ui_prefs(dir.path()), prefs);
}

#[test]
fn missing_last_used_fields_default() {
    let Ok(dir) = tempdir() else {
        return;
    };

    let json = r#"{ "theme": "light" }"#;
    assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());
    let prefs = load_ui_prefs(dir.path());
    assert_eq!(prefs.theme, Theme::Light);
    assert_eq!(prefs.last_entity_id, None);
    assert!(prefs.last_accounts_by_entity_kind.is_empty());
}
```

Update existing tests that build `UiPrefs { theme: Theme::Light }` to:

```rust
let prefs = UiPrefs {
    theme: Theme::Light,
    ..UiPrefs::default()
};
```

(and the equality check in `unknown_fields_are_tolerated` similarly).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oikonomia-core prefs -- --nocapture`  
Expected: FAIL (unknown fields / struct mismatch) until Step 3.

- [ ] **Step 3: Implement `LastRoleAccounts` + extend `UiPrefs`**

Replace the `UiPrefs` definition and `Default` in `prefs.rs`:

```rust
use std::collections::BTreeMap;

/// Last role-account picks for a single entity+kind tray post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LastRoleAccounts {
    pub category_account_id: Option<String>,
    pub wallet_account_id: Option<String>,
    pub payable_account_id: Option<String>,
    pub from_account_id: Option<String>,
    pub to_account_id: Option<String>,
}

/// Non-secret UI preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiPrefs {
    pub theme: Theme,
    /// Last entity used in the tray quick-add panel.
    pub last_entity_id: Option<String>,
    /// Map key: `"{entity_id}:{kind}"` (kind = expense|income|bill|transfer).
    pub last_accounts_by_entity_kind: BTreeMap<String, LastRoleAccounts>,
}
```

Remove `Copy` from the derive list (was on the old `UiPrefs`). Keep `Theme` as `Copy`.

Add a small pure helper (optional but preferred for IPC callers):

```rust
/// Build the map key for last-used accounts.
#[must_use]
pub fn last_accounts_key(entity_id: &str, kind: &str) -> String {
    format!("{entity_id}:{kind}")
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p oikonomia-core prefs`  
Expected: all prefs tests PASS (including pre-existing theme/corrupt/unknown).

- [ ] **Step 5: Commit**

```bash
git add crates/oikonomia-core/src/prefs.rs
git commit -m "feat(prefs): store last-used entity and accounts for tray quick-add"
```

---

### Task 2: IPC for prefs + window helpers

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register handlers)
- Modify: `apps/desktop/src-tauri/src/tray.rs` (export `show_main_window` already public; add `hide_quick_add` / `show_quick_add` used by commands — implement full window logic in Task 3; this task only adds thin command stubs that call tray helpers)

**Interfaces:**
- Consumes: `load_ui_prefs`, `save_ui_prefs`, `UiPrefs`, `LastRoleAccounts`, `last_accounts_key`
- Produces Tauri commands (register all in `invoke_handler!`):
  - `settings_get_ui_prefs() -> UiPrefs`
  - `settings_remember_quick_add(entity_id: String, kind: String, accounts: LastRoleAccounts) -> ()`
  - `open_main_window() -> ()` — calls `tray::show_main_window`
  - `quick_add_hide() -> ()` — calls `tray::hide_quick_add` (stub hide if window missing is OK)

- [ ] **Step 1: Add command implementations**

In `commands.rs`, import:

```rust
use oikonomia_core::prefs::{
    LastRoleAccounts, Theme, UiPrefs, last_accounts_key, load_ui_prefs, save_ui_prefs,
};
```

Add:

```rust
/// Full plaintext UI prefs (theme + tray last-used). Safe before unlock.
#[tauri::command]
pub fn settings_get_ui_prefs(state: State<'_, AppState>) -> UiPrefs {
    load_ui_prefs(state.data_dir())
}

/// Remember last entity + role accounts after a successful tray post.
#[tauri::command]
pub fn settings_remember_quick_add(
    state: State<'_, AppState>,
    entity_id: String,
    kind: String,
    accounts: LastRoleAccounts,
) -> CommandResult<()> {
    let mut prefs = load_ui_prefs(state.data_dir());
    prefs.last_entity_id = Some(entity_id.clone());
    prefs
        .last_accounts_by_entity_kind
        .insert(last_accounts_key(&entity_id, &kind), accounts);
    save_ui_prefs(state.data_dir(), &prefs)?;
    Ok(())
}

#[tauri::command]
pub fn open_main_window(app: tauri::AppHandle) {
    crate::tray::show_main_window(&app);
}

#[tauri::command]
pub fn quick_add_hide(app: tauri::AppHandle) {
    crate::tray::hide_quick_add(&app);
}
```

In `tray.rs`, add for now (Task 3 fills show/create):

```rust
pub fn hide_quick_add(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("quick-add") {
        if let Err(err) = window.hide() {
            log::warn!("failed to hide quick-add window: {err}");
        }
    }
}
```

Keep `settings_get_theme` / `settings_set_theme` working (they still only touch `theme`).

- [ ] **Step 2: Register commands in `lib.rs`**

Add to `generate_handler!`:

```rust
commands::settings_get_ui_prefs,
commands::settings_remember_quick_add,
commands::open_main_window,
commands::quick_add_hide,
```

- [ ] **Step 3: Compile check**

Run: `cargo check -p oikonomia`  
Expected: success (no missing symbols).

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/tray.rs
git commit -m "feat(desktop): IPC for quick-add prefs and window helpers"
```

---

### Task 3: Tray left-click + `quick-add` window shell

**Files:**
- Modify: `apps/desktop/src-tauri/src/tray.rs` (primary)
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/src/lib.rs` only if window-event handling needs a label branch (prefer: existing hide-on-close already correct for both windows)

**Interfaces:**
- Produces:
  - `tray::show_quick_add(app: &AppHandle, anchor: Option<tauri::Rect>)`
  - `tray::ensure_quick_add_window(app: &AppHandle) -> tauri::Result<WebviewWindow>`
  - Left primary-up click → `show_quick_add`
  - Right-click still opens native menu (with `show_menu_on_left_click(false)`, macOS/Windows show menu on right by default)
  - Window label **`quick-add`**, always-on-top, undecorated, not resizable, skip taskbar, initial size 420×110, starts hidden until show
- Consumes: same frontend URL as main (`WebviewUrl::App("index.html".into())`)

- [ ] **Step 1: Expand capabilities for the second window**

Replace `apps/desktop/src-tauri/capabilities/default.json` with:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Minimal local-only capabilities for Oikonomia v1 (no network).",
  "windows": ["main", "quick-add"],
  "permissions": [
    "core:default",
    "core:event:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-close",
    "core:window:allow-set-focus",
    "core:window:allow-set-size",
    "core:window:allow-set-position",
    "core:window:allow-outer-position",
    "core:window:allow-outer-size",
    "core:window:allow-is-visible"
  ]
}
```

(If a permission identifier fails clippy/build, drop only the failing one after checking `apps/desktop/src-tauri/gen/schemas` — do not add network.)

- [ ] **Step 2: Implement window ensure + show + position in `tray.rs`**

Full module shape (replace/extend existing file; keep `show_main_window` behavior):

```rust
//! System tray: left-click quick-add; menu Open / Quit; close hides windows.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

const QUICK_ADD_LABEL: &str = "quick-add";
const QUICK_ADD_WIDTH: f64 = 420.0;
const QUICK_ADD_HEIGHT: f64 = 110.0;

pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(err) = window.show() {
        log::warn!("failed to show main window: {err}");
    }
    if let Err(err) = window.set_focus() {
        log::warn!("failed to focus main window: {err}");
    }
}

pub fn hide_quick_add(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(QUICK_ADD_LABEL) {
        if let Err(err) = window.hide() {
            log::warn!("failed to hide quick-add window: {err}");
        }
    }
}

fn ensure_quick_add_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window(QUICK_ADD_LABEL) {
        return Ok(existing);
    }

    let window = WebviewWindowBuilder::new(app, QUICK_ADD_LABEL, WebviewUrl::App("index.html".into()))
        .title("Quick add")
        .inner_size(QUICK_ADD_WIDTH, QUICK_ADD_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .decorations(false)
        .build()?;

    Ok(window)
}

/// Position near tray click when possible; otherwise top-right of primary monitor work area is fine.
fn position_quick_add(window: &tauri::WebviewWindow, click: Option<PhysicalPosition<f64>>) {
    let Ok(outer) = window.outer_size() else {
        return;
    };
    let width = f64::from(outer.width);
    let height = f64::from(outer.height);

    let (x, y) = if let Some(pos) = click {
        (pos.x - width / 2.0, pos.y - height - 8.0)
    } else {
        (40.0, 40.0)
    };

    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
        x: x.round() as i32,
        y: y.round().max(0.0) as i32,
    }));
}

pub fn show_quick_add(app: &AppHandle, click: Option<PhysicalPosition<f64>>) {
    match ensure_quick_add_window(app) {
        Ok(window) => {
            position_quick_add(&window, click);
            if let Err(err) = window.show() {
                log::warn!("failed to show quick-add: {err}");
            }
            if let Err(err) = window.set_focus() {
                log::warn!("failed to focus quick-add: {err}");
            }
        }
        Err(err) => log::error!("failed to create quick-add window: {err}"),
    }
}

pub fn init(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Oikonomia", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Oikonomia", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Oikonomia")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                show_quick_add(tray.app_handle(), Some(position));
            }
        });

    let tray = if cfg!(target_os = "macos") {
        tray.icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
            .icon_as_template(true)
    } else if let Some(icon) = app.default_window_icon().cloned() {
        tray.icon(icon)
    } else {
        tray
    };

    tray.build(app)?;
    Ok(())
}
```

Adjust imports if the installed Tauri version names `WebviewWindowBuilder` under `tauri::WebviewWindowBuilder` instead of `tauri::webview::…` — match whatever `cargo check` requires.

- [ ] **Step 3: Build desktop crate**

Run: `cargo check -p oikonomia`  
Expected: success.

- [ ] **Step 4: Manual smoke (when you can run the app)**

Run: `cargo tauri dev --manifest-path apps/desktop/src-tauri/Cargo.toml`  
Check: left-click tray shows a blank/small window (frontend still mounts full App until Task 4); right-click still Open/Quit; closing the small window hides it (existing `CloseRequested` handler).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/tray.rs apps/desktop/src-tauri/capabilities/default.json
git commit -m "feat(desktop): tray left-click opens dedicated quick-add window"
```

---

### Task 4: Shared simple-entry helpers + frontend mount branch

**Files:**
- Create: `web/src/lib/simpleEntry.ts`
- Create: `web/src/lib/simpleEntry.test.ts`
- Modify: `web/src/lib/api.ts` (prefs + window commands)
- Modify: `web/src/main.tsx`
- Create: `web/src/QuickAddApp.tsx` (minimal locked shell first)

**Interfaces:**
- Produces pure functions (no React):
  - `export type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'`
  - `export type BillStatusTray = 'paid' | 'unpaid'`
  - `pickDefault(accounts, type, nameHints): string`
  - `accountsOf(accounts, types): Account[]`
  - `kindDefaultAccounts(kind, accounts): { categoryId, walletId, payableId, fromId, toId }`
  - `buildSimpleEntryInput(args): SimpleEntryInput` with `bill_status: kind === 'bill' ? billStatus : null` and **never** `pay_existing`
  - `lastAccountsMapKey(entityId, kind): string` → `` `${entityId}:${kind}` ``
- Produces API:
  - `api.getUiPrefs()`, `api.rememberQuickAdd(...)`, `api.openMainWindow()`, `api.quickAddHide()`
- Produces mount: label `quick-add` → `<QuickAddApp />`, else `<App />`

- [ ] **Step 1: Write failing unit tests**

`web/src/lib/simpleEntry.test.ts`:

```ts
import { describe, expect, test } from 'vitest'
import {
  buildSimpleEntryInput,
  kindDefaultAccounts,
  lastAccountsMapKey,
  type AccountLike,
} from './simpleEntry'

const accounts: AccountLike[] = [
  { id: 'e1', name: 'Food', account_type: 'expense', is_active: true },
  { id: 'i1', name: 'Salary', account_type: 'income', is_active: true },
  { id: 'a1', name: 'Checking', account_type: 'asset', is_active: true },
  { id: 'a2', name: 'Savings', account_type: 'asset', is_active: true },
  { id: 'l1', name: 'Bills Payable', account_type: 'liability', is_active: true },
]

describe('lastAccountsMapKey', () => {
  test('joins entity and kind', () => {
    expect(lastAccountsMapKey('ent', 'expense')).toBe('ent:expense')
  })
})

describe('kindDefaultAccounts', () => {
  test('expense picks expense + checking', () => {
    const d = kindDefaultAccounts('expense', accounts)
    expect(d.categoryId).toBe('e1')
    expect(d.walletId).toBe('a1')
  })

  test('transfer picks two assets', () => {
    const d = kindDefaultAccounts('transfer', accounts)
    expect(d.fromId).toBe('a1')
    expect(d.toId).toBe('a2')
  })
})

describe('buildSimpleEntryInput', () => {
  test('maps expense fields and null bill_status', () => {
    const input = buildSimpleEntryInput({
      entityId: 'ent',
      kind: 'expense',
      billStatus: 'unpaid',
      entryDate: '2026-08-11',
      description: ' Coffee ',
      amountMinor: 250,
      categoryId: 'e1',
      walletId: 'a1',
      payableId: '',
      fromId: '',
      toId: '',
    })
    expect(input.kind).toBe('expense')
    expect(input.bill_status).toBeNull()
    expect(input.description).toBe('Coffee')
    expect(input.category_account_id).toBe('e1')
    expect(input.wallet_account_id).toBe('a1')
  })

  test('bill includes unpaid status only', () => {
    const input = buildSimpleEntryInput({
      entityId: 'ent',
      kind: 'bill',
      billStatus: 'unpaid',
      entryDate: '2026-08-11',
      description: 'Gas',
      amountMinor: 1000,
      categoryId: 'e1',
      walletId: 'a1',
      payableId: 'l1',
      fromId: '',
      toId: '',
    })
    expect(input.bill_status).toBe('unpaid')
    expect(input.payable_account_id).toBe('l1')
  })
})
```

- [ ] **Step 2: Run tests — expect FAIL**

Run: `cd web && npm run test -- src/lib/simpleEntry.test.ts`  
Expected: FAIL module not found.

- [ ] **Step 3: Implement `simpleEntry.ts`**

Extract logic equivalent to TransactionsPage’s `pickDefault` / `applyKindDefaults` / post payload (mirror name hints exactly). Export types compatible with `Account` from `api.ts` (either import `Account` or use a minimal `AccountLike`).

```ts
import type { SimpleEntryInput } from './api'

export type EntryKind = 'expense' | 'income' | 'bill' | 'transfer'
export type BillStatusTray = 'paid' | 'unpaid'

export type AccountLike = {
  id: string
  name: string
  account_type: 'asset' | 'liability' | 'equity' | 'income' | 'expense'
  is_active: boolean
}

export function lastAccountsMapKey(entityId: string, kind: EntryKind | string): string {
  return `${entityId}:${kind}`
}

export function pickDefault(
  accounts: AccountLike[],
  type: AccountLike['account_type'],
  nameHints: string[] = [],
): string {
  const active = accounts.filter((a) => a.is_active && a.account_type === type)
  for (const hint of nameHints) {
    const found = active.find((a) => a.name.toLowerCase().includes(hint.toLowerCase()))
    if (found) return found.id
  }
  return active[0]?.id ?? ''
}

export function accountsOf(
  accounts: AccountLike[],
  types: AccountLike['account_type'][],
): AccountLike[] {
  return accounts.filter((a) => a.is_active && types.includes(a.account_type))
}

export function kindDefaultAccounts(kind: EntryKind, list: AccountLike[]) {
  if (kind === 'expense') {
    return {
      categoryId: pickDefault(list, 'expense', ['food', 'utilities', 'bills', 'other']),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: '',
      fromId: '',
      toId: '',
    }
  }
  if (kind === 'income') {
    return {
      categoryId: pickDefault(list, 'income', ['salary', 'sales', 'freelance']),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: '',
      fromId: '',
      toId: '',
    }
  }
  if (kind === 'bill') {
    return {
      categoryId: pickDefault(list, 'expense', [
        'utilities',
        'bills',
        'housing',
        'subscription',
        'rent',
      ]),
      walletId: pickDefault(list, 'asset', ['checking', 'bank', 'cash']),
      payableId: pickDefault(list, 'liability', [
        'bills payable',
        'accounts payable',
        'payable',
      ]),
      fromId: '',
      toId: '',
    }
  }
  const fromId = pickDefault(list, 'asset', ['checking', 'bank'])
  const savings = pickDefault(list, 'asset', ['savings', 'cash'])
  return {
    categoryId: '',
    walletId: '',
    payableId: '',
    fromId,
    toId: savings || pickDefault(list, 'asset', []),
  }
}

export function buildSimpleEntryInput(args: {
  entityId: string
  kind: EntryKind
  billStatus: BillStatusTray
  entryDate: string
  description: string
  amountMinor: number
  categoryId: string
  walletId: string
  payableId: string
  fromId: string
  toId: string
}): SimpleEntryInput {
  return {
    entity_id: args.entityId,
    kind: args.kind,
    bill_status: args.kind === 'bill' ? args.billStatus : null,
    entry_date: args.entryDate,
    description: args.description.trim(),
    reference: null,
    amount_minor: args.amountMinor,
    category_account_id: args.categoryId || null,
    wallet_account_id: args.walletId || null,
    payable_account_id: args.payableId || null,
    from_account_id: args.fromId || null,
    to_account_id: args.toId || null,
  }
}
```

- [ ] **Step 4: Extend `api.ts`**

Add types + methods (serde field names match Rust snake_case via existing `call` pattern — check how theme is called). Looking at existing code, command args are camelCase or snake_case per each command. Match Rust param names: Tauri 2 renames typically to camelCase in JS — use the same style as `settings_set_theme` / `entityId` elsewhere.

```ts
export type LastRoleAccounts = {
  category_account_id: string | null
  wallet_account_id: string | null
  payable_account_id: string | null
  from_account_id: string | null
  to_account_id: string | null
}

export type UiPrefs = {
  theme: Theme
  last_entity_id: string | null
  last_accounts_by_entity_kind: Record<string, LastRoleAccounts>
}

// inside api object:
getUiPrefs: () => call<UiPrefs>('settings_get_ui_prefs'),
rememberQuickAdd: (entityId: string, kind: string, accounts: LastRoleAccounts) =>
  call<void>('settings_remember_quick_add', {
    entityId,
    kind,
    accounts,
  }),
openMainWindow: () => call<void>('open_main_window'),
quickAddHide: () => call<void>('quick_add_hide'),
```

If invoke arg rename fails at runtime, switch keys to `entity_id` to match Rust exactly (desktop commands use snake_case param names — Tauri serializes with the Rust names unless `rename_all` is set). **Use Rust parameter names in the payload:**

```ts
rememberQuickAdd: (entityId: string, kind: string, accounts: LastRoleAccounts) =>
  call<void>('settings_remember_quick_add', {
    entity_id: entityId,
    kind,
    accounts,
  }),
```

(Confirm against existing commands: `entry_list` uses `entityId` in api.ts — follow whatever pattern already works for multi-arg commands in this codebase.)

- [ ] **Step 5: `QuickAddApp.tsx` minimal shell + `main.tsx` branch**

`QuickAddApp.tsx` (first cut — locked / loading / placeholder unlocked):

```tsx
import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api } from './lib/api'
import { isTauri, vaultStatus, type VaultStatus } from './lib/tauri'
import { Button } from './components/ui'

export default function QuickAddApp() {
  const [status, setStatus] = useState<VaultStatus | null>(null)
  const [dark, setDark] = useState(true)

  useEffect(() => {
    void api.getTheme().then((t) => setDark(t === 'dark')).catch(() => undefined)
  }, [])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  useEffect(() => {
    void vaultStatus().then(setStatus).catch(() => setStatus('locked'))
  }, [])

  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void listen('vault-locked', () => setStatus('locked')).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void api.quickAddHide()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (status === null) {
    return (
      <div className="flex h-screen items-center justify-center bg-[var(--color-bg)] text-xs text-[var(--color-muted)]">
        Loading…
      </div>
    )
  }

  if (status !== 'unlocked') {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-3 bg-[var(--color-bg)] px-4 text-center">
        <p className="text-sm text-[var(--color-fg)]">Vault is locked</p>
        <p className="text-xs text-[var(--color-muted)]">Open Oikonomia to unlock, then try again.</p>
        <Button size="sm" onClick={() => void api.openMainWindow()}>
          Open Oikonomia
        </Button>
      </div>
    )
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[var(--color-bg)] text-xs text-[var(--color-muted)]">
      Quick add (form in next task)
    </div>
  )
}
```

`main.tsx`:

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import './index.css'
import App from './App.tsx'
import QuickAddApp from './QuickAddApp.tsx'
import { isTauri } from './lib/tauri'

const root = document.getElementById('root')
if (!root) {
  throw new Error('root element missing')
}

document.documentElement.classList.add('dark')

async function mount() {
  let isQuickAdd = false
  if (isTauri()) {
    try {
      isQuickAdd = getCurrentWindow().label === 'quick-add'
    } catch {
      isQuickAdd = false
    }
  }

  createRoot(root).render(
    <StrictMode>{isQuickAdd ? <QuickAddApp /> : <App />}</StrictMode>,
  )
}

void mount()
```

- [ ] **Step 6: Run frontend gate**

Run: `cd web && npm run test && npm run build && npm run lint`  
Expected: pass.

- [ ] **Step 7: Commit**

```bash
git add web/src/lib/simpleEntry.ts web/src/lib/simpleEntry.test.ts web/src/lib/api.ts web/src/main.tsx web/src/QuickAddApp.tsx
git commit -m "feat(web): mount QuickAddApp and shared simple-entry helpers"
```

---

### Task 5: Quick-add form + simple post + last-used defaults

**Files:**
- Create: `web/src/pages/QuickAddPage.tsx`
- Modify: `web/src/QuickAddApp.tsx` (render page when unlocked; success state; activity heartbeat; blur hide)
- Optionally refactor TransactionsPage later to import `pickDefault` from `simpleEntry` — **not required** for v1

**Interfaces:**
- Consumes: `api.entityList`, `accountList`, `entryPostSimple`, `getUiPrefs`, `rememberQuickAdd`, `quickAddHide`, `openMainWindow`, helpers from `simpleEntry`
- UI: kind `Segmented`, amount `Input`, role `Select`s, `DateInput`, memo `Input`, Add `Button`, entity `Select` if `entities.length > 1`
- Bill: small unpaid/paid toggle; default **unpaid**
- Success: show “Saved” ~1000ms then `quickAddHide` and reset
- On open / when becoming unlocked: load prefs + entities + accounts; apply last-used

- [ ] **Step 1: Implement `QuickAddPage` form + post**

Implement a compact layout using existing UI primitives (`Segmented`, `Input`, `Select`, `Button`, `DateInput`, `ErrorBanner`). Structure:

- State mirrors Transactions form fields but `billStatus: 'paid' | 'unpaid'` only, default `'unpaid'`.
- `load()` on mount / entity change: accounts + prefs-driven defaults.
- Prefer last_accounts map entry if all referenced ids still exist in `accounts`; else `kindDefaultAccounts`.
- `onSubmit`: `parseMajorToMinor` → `buildSimpleEntryInput` → `entryPostSimple` → `rememberQuickAdd` with `LastRoleAccounts` from current ids → call `onPosted({ kind, amountMinor, currency })`.
- No entities: empty state “Create an entity in Oikonomia” + button `openMainWindow`.

Key remember payload:

```ts
await api.rememberQuickAdd(entity.id, kind, {
  category_account_id: categoryId || null,
  wallet_account_id: walletId || null,
  payable_account_id: payableId || null,
  from_account_id: fromId || null,
  to_account_id: toId || null,
})
```

- [ ] **Step 2: Wire success + heartbeat + blur in `QuickAddApp`**

```tsx
// sketch
const [phase, setPhase] = useState<'form' | 'success'>('form')
const [successLabel, setSuccessLabel] = useState('')
const busyRef = useRef(false)

// activity heartbeat like App.tsx (throttle vaultStatus every 60s on pointer/key)
// blur: if (!busyRef.current) void api.quickAddHide()
// onPosted: set success label, phase success, setTimeout 1000 → hide + reset phase form
```

When `status` flips to locked: clear phase to form and do not keep draft (page remount via `key={status}` is fine).

- [ ] **Step 3: Build + unit tests**

Run: `cd web && npm run test && npm run build && npm run lint`  
Expected: pass.

- [ ] **Step 4: Manual check**

With vault unlocked: left-click tray → form → post expense → “Saved” → window hides → reopen shows last accounts.

- [ ] **Step 5: Commit**

```bash
git add web/src/pages/QuickAddPage.tsx web/src/QuickAddApp.tsx
git commit -m "feat(web): tray quick-add form posts simple entries"
```

---

### Task 6: Document drop + expand review + resize window

**Files:**
- Modify: `web/src/pages/QuickAddPage.tsx`
- Modify: `apps/desktop/src-tauri/src/tray.rs` **or** frontend `getCurrentWindow().setSize` for height 110 ↔ 320
- Reuse patterns from `DocumentDropZone.tsx` (path drops via `getCurrentWebview().onDragDropEvent`, browser `File` drops, 8 MB guard)

**Interfaces:**
- On drop: set analyzing → `documentAnalyze` / `documentAnalyzePath` → expand UI with suggestion fields → Confirm uses `entryPostSimpleWithDocument` / `…Path` → remember prefs → success.
- Cancel clears pending doc and collapses.
- While analyzing/posting set `busyRef` so blur does not dismiss.
- After expand, call `getCurrentWindow().setSize(new LogicalSize(420, 320))`; collapse back to 110.

- [ ] **Step 1: Add pending doc state + drop handlers**

Mirror TransactionsPage `applySuggestion` but force tray bill status to unpaid/paid only (`s.bill_unpaid ? 'unpaid' : 'paid'`). Reuse `fileToBase64`, `mimeFromName`, `currencyFractionDigits` amount display.

- [ ] **Step 2: Expanded review UI**

Show filename, notes, editable fields (already shared with form), **Confirm & save** / **Cancel**. Errors stay on panel.

- [ ] **Step 3: Resize helper**

```ts
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'

async function setQuickAddHeight(height: number) {
  if (!isTauri()) return
  await getCurrentWindow().setSize(new LogicalSize(420, height))
}
```

Call on enter/leave review mode and on hide reset.

- [ ] **Step 4: Frontend gate**

Run: `cd web && npm run test && npm run build && npm run lint`

- [ ] **Step 5: Manual document path**

Drop a PDF/receipt on the panel → expand → confirm → entry + document in full app Transactions/Documents.

- [ ] **Step 6: Commit**

```bash
git add web/src/pages/QuickAddPage.tsx web/src/QuickAddApp.tsx
git commit -m "feat(web): document drop and review in tray quick-add"
```

---

### Task 7: Polish + regression gate

**Files:**
- Touch as needed: `QuickAddApp.tsx`, `QuickAddPage.tsx`, `tray.rs`, `index.css` (only if panel needs `overflow-hidden` / drag region)
- Modify: `AGENTS.md` one line under invariants/layout if tray quick-add should be documented (optional short bullet)

**Checklist:**

- [ ] **Step 1: Theme parity** — light theme in prefs flips panel correctly on open.
- [ ] **Step 2: Locked mid-flight** — unlock, open panel, wait for auto-lock (or call lock from main) → panel shows locked UI, draft gone.
- [ ] **Step 3: All four kinds** — expense, income, bill unpaid/paid, transfer post successfully.
- [ ] **Step 4: Right-click menu** — Open focuses main; Quit exits.
- [ ] **Step 5: Full automated gate**

```bash
cargo fmt --all
cargo test -p oikonomia-core
cargo clippy --all-targets --all-features -- -D warnings
cd web && npm run test && npm run build && npm run lint
```

Expected: all green.

- [ ] **Step 6: Final commit if polish diffs remain**

```bash
git add -A
git commit -m "fix: tray quick-add polish and regression gate"
```

---

## Spec coverage (self-review)

| Spec requirement | Task |
|------------------|------|
| Left-click quick-add / right-click menu | Task 3 |
| Second window `quick-add`, small, always-on-top | Task 3 |
| Same SPA, branch on label | Task 4 |
| All simple kinds | Task 5 |
| Locked message + Open | Task 4–5 |
| Document expand review | Task 6 |
| Last-used entity + accounts | Task 1, 2, 5 |
| Success flash then hide | Task 5 |
| Bill unpaid default + toggle; no pay_existing | Task 4 helpers + Task 5 |
| `vault-locked` clears draft | Task 4–5 |
| Activity heartbeat | Task 5 |
| Prefs tests | Task 1 |
| No new ledger rules | All tasks use existing commands |

**Resolved open details from spec:**

- Memo line always visible (second micro-row).
- Blur-to-dismiss when not busy.
- `pay_existing` excluded from tray.

**Placeholder scan:** none intentional.  
**Type consistency:** `LastRoleAccounts` / map key `entity:kind` / window label `quick-add` used uniformly.
