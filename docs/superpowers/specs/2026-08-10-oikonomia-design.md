# Oikonomia — Product & Architecture Design

**Status:** Ready for user approval  
**Date:** 2026-08-10  
**Repo:** `oikonomia` (greenfield Rust crate → Tauri app)

---

## 1. Vision

**Oikonomia** is a local-only desktop app for tracking **personal and company** finances with **industry-standard double-entry bookkeeping**, a **Stripe-grade web UI** (dark mode first), and **full encryption at rest** so a stolen disk or backup does not leak books.

No cloud, no accounts, no network required. One machine, one encrypted vault, unlock with a master password.

---

## 2. Confirmed decisions

| Topic | Decision |
|-------|----------|
| Packaging | **Tauri 2** desktop (Rust + webview) |
| Frontend | **React + TypeScript + Vite + Tailwind + shadcn/ui** |
| IPC | **Tauri commands** (no HTTP port) |
| Storage | **SQLCipher** (AES-256 encrypted SQLite) |
| Unlock | **Master password → Argon2id → DB key** |
| Accounting | **Double-entry** ledger |
| Books | **Multi-entity** (Personal, Company A, …) |
| Currency | **One base currency per entity** |
| v1 scope | Core ledger + trial balance, P&L, balance sheet |
| Language | English UI; locale-aware number/date formatting |
| Out of v1 | Recurring, attachments, budgets, CSV, invoicing, multi-currency, sync, Greek UI |

---

## 3. Architecture overview

```
┌─────────────────────────────────────────────────────────┐
│  Tauri Webview (React SPA)                              │
│  shadcn + Tailwind · dark/light · entity switcher       │
└──────────────────────────┬──────────────────────────────┘
                           │ invoke("command", payload)
                           ▼
┌─────────────────────────────────────────────────────────┐
│  Rust core (Tauri commands)                             │
│  ┌────────────┐  ┌──────────────┐  ┌─────────────────┐  │
│  │ vault/auth │  │  domain/svc  │  │  reports        │  │
│  │ lock/unlock│→ │  entities    │→ │  TB / P&L / BS  │  │
│  │ argon2     │  │  accounts    │  │                 │  │
│  └─────┬──────┘  │  journals    │  └─────────────────┘  │
│        │         └──────┬───────┘                       │
│        ▼                ▼                               │
│  ┌──────────────────────────────────────────────────┐   │
│  │  repository (rusqlite + SQLCipher)               │   │
│  └──────────────────────┬───────────────────────────┘   │
└─────────────────────────┼───────────────────────────────┘
                          ▼
              ~/.…/oikonomia/vault.db  (ciphertext at rest)
```

**Workspace layout (proposed):**

```
oikonomia/
  apps/desktop/          # Tauri shell (tauri.conf, icons)
  crates/core/           # domain, crypto, repository, reports (library)
  crates/app/            # Tauri command handlers, wiring
  web/                   # React SPA
  docs/                  # design + plans
  Cargo.toml             # workspace
  rustfmt.toml
```

- `crates/core` is pure library: unit/property-tested without UI.
- `crates/app` is thin: serialize errors, hold `AppState` (locked/unlocked).
- `web/` never talks to the filesystem or SQL directly.

---

## 4. Domain model (double-entry)

### 4.1 Core types

```text
Vault
  vault_id, kdf_params (argon2), created_at, schema_version

Entity (book)
  id, name, base_currency (ISO 4217), fiscal_year_start_month,
  chart_template (personal | company | blank), created_at, archived_at?

Account
  id, entity_id, code, name,
  account_type: Asset | Liability | Equity | Income | Expense,
  parent_id?, is_active, is_system (e.g. Opening Balances equity),
  sort_order

JournalEntry
  id, entity_id, entry_date (calendar date), description,
  reference? (check #, invoice #),
  status: Draft | Posted,
  created_at, posted_at?, voided_by_entry_id?

JournalLine
  id, entry_id, account_id,
  debit_minor: i64,   # ≥ 0
  credit_minor: i64,  # ≥ 0
  memo?
```

### 4.2 Money

- Store **integer minor units** (`i64`) only. Never `f64` for money.
- Display via currency exponent (EUR/USD = 2). Entity currency is source of truth.
- Domain type e.g. `Money { amount_minor: i64 }` with checked arithmetic.

### 4.3 Invariants (enforced in Rust, not only UI)

1. **Balanced entry:** for every posted entry, `Σ debit_minor == Σ credit_minor`.
2. **Line shape:** each line has exactly one of debit or credit non-zero (or both zero forbidden); recommend exclusive debit XOR credit.
3. **Minimum lines:** posted entry has ≥ 2 lines.
4. **Same entity:** all lines’ accounts belong to the entry’s entity.
5. **No float:** all amounts integer.
6. **Posted immutability:** posted entries are not edited in place; corrections via **void + reverse** (or reverse entry) to preserve audit trail.
7. **Drafts:** optional drafts for multi-step UX; only posted entries affect balances/reports.

### 4.4 Chart of accounts templates

**Personal (starter):**  
Assets: Cash, Checking, Savings, Investments  
Liabilities: Credit Card, Loans  
Equity: Opening Balances, Owner Equity  
Income: Salary, Freelance, Interest, Other Income  
Expenses: Housing, Food, Transport, Utilities, Health, Subscriptions, Entertainment, Taxes, Other

**Company (starter):**  
Assets: Cash, Bank, Accounts Receivable, Equipment  
Liabilities: Accounts Payable, Credit Card, Loans, Taxes Payable  
Equity: Owner Capital, Retained Earnings, Opening Balances  
Income: Sales / Services, Other Income  
Expenses: COGS (optional), Payroll, Rent, Software, Marketing, Professional Fees, Travel, Taxes, Other OpEx

Users can rename/add/archive accounts; system accounts (Opening Balances) cannot be deleted if referenced.

### 4.5 Opening balances

On entity create (or settings): single balanced journal  
`Dr Asset accounts … / Cr Opening Balances (Equity)` (or reverse for credit-card opening).

### 4.6 Balances & reports (derived)

| Report | Rule (simplified) |
|--------|-------------------|
| **Account balance** | Assets/Expenses: debits − credits; Liabilities/Equity/Income: credits − debits |
| **Trial balance** | All accounts with period activity or non-zero balance; totals match |
| **P&L** | Income − Expenses for date range → Net income |
| **Balance sheet** | Assets = Liabilities + Equity **as of date** (includes closed P&L into equity for as-of correctness) |

**Closing:** v1 can compute balance sheet “as of” by treating YTD net income as equity line “Net Income (current)” without permanent year-end close workflow (document this; full fiscal close is v1.x).

---

## 5. Security & encryption

### 5.1 Threat model (v1)

**Protect against:** stolen laptop disk, cloud backup of home folder, casual file browsing, leftover plaintext DB.  
**Not protecting against:** malware with same-user access while vault is unlocked, evil maid with hardware keyloggers, memory forensics while unlocked.

### 5.2 Vault lifecycle

1. **First run:** user sets master password (strength meter, min length e.g. 12). Generate random salt; Argon2id derive key; create SQLCipher DB with that key; store public header (salt, argon2 params, schema version) adjacent or in DB pragma metadata.
2. **Unlock:** password → derive key → open SQLCipher → hold key in memory only inside process state.
3. **Lock / quit:** close DB connection; zeroize key material (`zeroize` crate); UI returns to unlock screen.
4. **Change password:** rekey SQLCipher (or export/re-encrypt) after verifying old password.
5. **Auto-lock:** configurable idle timeout (default 15 min); lock on sleep if easy via Tauri events.

### 5.3 Crypto choices

| Concern | Choice |
|---------|--------|
| At-rest DB | SQLCipher default AES-256-CBC (or AES-256-GCM if available in chosen build) |
| KDF | Argon2id (params tuned for ~200–500ms on modern laptop; document params) |
| Secret handling | `zeroize`, avoid logging amounts in release logs if ever added |
| Password storage | Never store password or raw key on disk |

### 5.4 Tauri capabilities

- **No network** permission in v1 `capabilities`.
- Filesystem access limited to app data directory.
- Frontend CSP strict; no remote scripts.

### 5.5 Data location

- macOS: `~/Library/Application Support/oikonomia/`
- Linux: `$XDG_DATA_HOME/oikonomia/`
- Windows: `%APPDATA%\oikonomia\`
- Single primary file: `vault.db` (+ optional `-wal`/`-shm` which SQLCipher also protects when configured correctly — verify WAL encryption behavior in integration tests).

---

## 6. Backend API surface (Tauri commands)

All commands return `Result<T, AppError>` with stable error codes for the UI.

**Vault**
- `vault_status` → `Uninitialized | Locked | Unlocked`
- `vault_init { password }`
- `vault_unlock { password }`
- `vault_lock`
- `vault_change_password { old, new }`

**Entities**
- `entity_list` / `entity_create` / `entity_update` / `entity_archive`
- `entity_set_active` (session focus)

**Accounts**
- `account_list { entity_id }`
- `account_create` / `account_update` / `account_archive`
- `account_register { account_id, from, to }` → lines + running balance

**Journal**
- `entry_list { entity_id, from?, to?, account_id?, q? }`
- `entry_get { id }`
- `entry_create_draft` / `entry_update_draft` / `entry_post` / `entry_void`
- Or simpler v1: only posted create + void (no drafts) if we want less surface — **recommend drafts for multi-line form UX**

**Reports**
- `report_trial_balance { entity_id, as_of }`
- `report_pnl { entity_id, from, to }`
- `report_balance_sheet { entity_id, as_of }`

**Dashboard**
- `dashboard_summary { entity_id, from, to }` → cash-like assets total, income, expenses, recent entries

**Settings**
- `settings_get` / `settings_set` (theme preference, lock timeout) — theme can live in frontend localStorage; lock timeout must be in Rust

**Guards:** every data command requires `Unlocked` state or returns `VaultLocked`.

---

## 7. Frontend UX (Stripe-inspired)

### 7.1 Visual system

- **Dark mode default**; light mode toggle.
- Design tokens: near-black canvas (`zinc-950`), elevated surfaces (`zinc-900`), hairline borders (`zinc-800`), muted secondary text, accent indigo/violet (Stripe-adjacent, not neon).
- Typography: Inter or system UI stack; tabular nums for money.
- Density: comfortable tables, clear hierarchy, empty states with one primary CTA.
- Components: shadcn Button, Input, Table, Dialog, Dropdown, Select, Tabs, Sheet, Toast, Command palette optional later.

### 7.2 Information architecture

```
[Entity switcher ▾]     Oikonomia                    [Theme] [Lock]
────────────────────────────────────────────────────────────────
Dashboard
Transactions
Accounts
Reports ▸ Trial balance | P&L | Balance sheet
Settings ▸ Entities | Security | Appearance
```

### 7.3 Key flows

1. **First run:** welcome → set password → create first entity (template + currency) → dashboard empty state.
2. **Add expense:** Transactions → New entry → date, description, Dr Expense / Cr Bank → Post.
3. **Transfer:** Dr Savings / Cr Checking (same entity).
4. **Reports:** pick range/as-of → printable-friendly layout (browser print later).
5. **Lock:** header button or idle → unlock modal only.

### 7.4 Accessibility & polish

- Keyboard-friendly forms (tab order, Enter to post with confirm).
- Money inputs: parse locale, store minor units.
- Destructive actions: confirm dialogs.
- No emoji in product chrome (per house rules).

---

## 8. Error handling

- Domain errors: `UnbalancedEntry`, `AccountWrongEntity`, `EntryNotDraft`, `VaultLocked`, `InvalidPassword`, `AccountInUse`, …
- Map to user-facing strings in the web layer (or Rust i18n keys).
- Never include password material in errors.
- DB corruption / wrong key: clear “Could not unlock vault” without distinguishing if that helps security (optional: constant-time verify; practical v1 is fine to just fail open).

---

## 9. Testing strategy

| Layer | What |
|-------|------|
| Unit | Money math, account normal balance, report aggregations |
| Property / invariant | Random balanced entries keep trial balance; unbalanced rejected |
| Integration | Temp SQLCipher vault: init → unlock → CRUD → lock → unlock → data remains |
| Crypto smoke | Wrong password fails; file entropy looks non-plaintext |
| UI (later) | Playwright smoke optional post-MVP; manual checklist for v1 |

CI (when added): `cargo fmt`, `clippy -D warnings`, `cargo test`, frontend `tsc` + vitest for pure formatters.

---

## 10. Dependencies (initial)

**Rust (indicative):**  
`tauri` 2, `serde`, `thiserror`, `anyhow` (app boundary only), `rusqlite` + SQLCipher feature, `argon2`, `zeroize`, `uuid`, `time` or `chrono`, `rust_decimal` optional (prefer raw i64), `tracing`.

**Web:**  
`react`, `react-router`, `@tauri-apps/api`, `tailwindcss`, `shadcn` stack, `zod` for form validation, `date-fns` or `@internationalized/date`.

**Supply chain:** check RUSTSEC before pin; prefer maintained crates; no unnecessary deps.

---

## 11. Key decisions (rationale)

1. **Double-entry from day one** — company-grade correctness; personal is a simplified CoA, not a different engine.
2. **Multi-entity** — clean separation of personal vs company funds; no accidental mixing.
3. **Tauri IPC not HTTP** — no open port; smaller local attack surface.
4. **SQLCipher** — industry-standard encrypted SQLite; single file vault; queryable after unlock.
5. **Integer minor units** — avoid float money bugs forever.
6. **Posted immutability + void** — audit trail suitable for bookkeeping.
7. **v1 ruthlessly small** — ledger + three reports + Stripe UI + encryption; expand later.
8. **React/shadcn** — fastest path to Stripe-like polish and component quality.

---

## 12. Implementation PR plan

| PR | Title | Delivers | Depends |
|----|-------|----------|---------|
| **PR1** | Workspace scaffold + Rust style | Cargo workspace, rustfmt/clippy lints, empty Tauri+Vite shell, AGENTS.md | — |
| **PR2** | Vault crypto + encrypted DB | Init/unlock/lock/change-password; SQLCipher integration tests | PR1 |
| **PR3** | Domain model + accounts + entities | Types, migrations, CoA templates, entity CRUD | PR2 |
| **PR4** | Journal entries + invariants | Create/post/void, register views, property tests | PR3 |
| **PR5** | Reports engine | Trial balance, P&L, balance sheet + unit tests | PR4 |
| **PR6** | Design system + app shell | Dark/light theme, layout, unlock, entity switcher (mocked or live) | PR1–2 |
| **PR7** | Wire ledger UI | Transactions, accounts, forms, dashboard | PR4–6 |
| **PR8** | Wire reports UI + polish | Report pages, empty states, auto-lock, capability audit | PR5–7 |
| **PR9** | Hardening & docs | README, threat model notes, release build checklist | PR8 |

PRs 3–5 are backend-heavy and can overlap slightly with PR6 once PR2 lands. PR7 must not invent parallel business logic in TypeScript.

---

## 13. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| SQLCipher + rusqlite build pain on macOS | Prototype in PR2 early; pin known-good feature flags; document brew/openssl if needed |
| Balance sheet “net income” edge cases | Explicit report spec + golden-file fixtures |
| UI scope creep | v1 checklist only; park features in backlog section |
| Password loss = data loss | Clear warning on first run; v1.1 optional recovery key export (out of scope now) |

---

## 14. Future backlog (not v1)

- CSV import/export, encrypted backup export
- Attachments (encrypted blobs)
- Recurring templates, budgets
- Multi-currency + FX
- Invoicing / AR-AP subledgers
- Greek UI (i18n)
- Optional recovery key
- Full fiscal year close workflow

---

## 15. Success criteria

- [ ] Vault file is unreadable without password (manual hex/string check)
- [ ] Unbalanced journal rejected by core
- [ ] Sample company + personal books: TB balances; BS equation holds; P&L matches hand calc
- [ ] Dark mode UI feels Stripe-adjacent (design review)
- [ ] No network capability in shipped Tauri config
- [ ] `cargo test` + clippy clean on core

---

## 16. Approval gate

Approve this design to proceed to:
1. Formal design doc under `docs/` (optional mirror of this plan)
2. Detailed implementation plan (task breakdown for PR1+)
3. Implementation starting at PR1 scaffold
