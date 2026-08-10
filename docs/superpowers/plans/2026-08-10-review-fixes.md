# Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix every finding of the 2026-08-10 top-to-bottom review (F1–F12 + hygiene): correct report math, transactional writes, entity deletion, dependency vulnerability, Rust-side auto-lock, password change, frontend logic leaks — each with regression tests.

**Architecture:** All business-rule fixes land in `oikonomia-core` (reports SQL rewrite, transactions, simple-entry builder, password rekey). The Tauri shell gains an auto-lock watchdog thread, async document analysis, and two new commands. The frontend loses its duplicated business logic (debit/credit construction, void filtering, lock timing) and gains change-password UI.

**Tech Stack:** Rust 2024 / rusqlite 0.40 (SQLCipher) / Tauri 2 / React 19 + TS / vitest for frontend unit tests.

## Global Constraints

- Money is integer minor units (`i64`); never `f64` for currency (AGENTS.md).
- Business rules live in `oikonomia-core`, not TypeScript (CLAUDE.md: "All the business logic needs to live in Rust side").
- Workspace lints: `unwrap_used = deny`, `panic = deny`, `allow_attributes = deny` (use `#[expect(...)]`, never `#[allow(...)]`); test files that use `.expect()` start with `#![expect(clippy::expect_used)]`.
- `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings` must be clean at the end (Task 11 gate).
- No emojis in code, UI chrome, or commits. No `Co-Authored-By` lines in commits (home AGENTS.md overrides harness default).
- Commits are small, one per task, message explains why.
- No network permission may be added to Tauri capabilities; CSP stays `'self'` + IPC only.
- The four review probe tests live at `<scratchpad>/review_probe.rs` and are the acceptance anchor for Tasks 2–3.

---

### Task 1: Baseline commit + failing regression tests

**Files:**
- Create: `crates/oikonomia-core/tests/report_correctness.rs`
- Create: `crates/oikonomia-core/tests/documents_flow.rs`

**Interfaces:**
- Produces: test helpers `setup_vault() -> (TempDir, Vault)`, `post_expense(conn, entity_id, date, minor) -> String` reused as the pattern for later test files (each file self-contains its helpers; no shared test crate).

- [ ] **Step 1: Commit the pre-fix state** (repo currently has zero commits)

```bash
git add -A && git commit -m "chore: initial import of oikonomia v0.1 (pre-review-fix state)"
```

- [ ] **Step 2: Add failing report tests** — port the four probes from the review scratchpad into `tests/report_correctness.rs` (P&L range, TB as-of, BS balance for past as-of) and `tests/documents_flow.rs` (delete entity with linked document). Both files start with `#![expect(clippy::expect_used)]`. Also add to `report_correctness.rs`:

```rust
#[test]
fn voided_entry_leaves_no_trace_in_trial_balance() {
    // post 1_000 on 2026-01-15, void it, then:
    let tb = trial_balance(conn, entity_id, "2026-12-31").expect("tb");
    assert!(tb.lines.is_empty(), "voided pair must not appear as gross activity: {:?}", tb.lines);
}

#[test]
fn randomized_entries_keep_trial_balance_balanced() {
    // seeded rand::rngs::StdRng (rand is already a core dependency), 50 random
    // balanced 2-line entries across accounts/dates in 2026, assert
    // tb.total_debits == tb.total_credits and BS balances at 3 as-of dates.
}
```

- [ ] **Step 3: Run and record failures**

Run: `cargo test -p oikonomia-core --test report_correctness --test documents_flow`
Expected: FAIL — pnl range (3000 vs 2000), tb as-of (3000 vs 1000), bs imbalance (−3000 vs −1000), voided trace, FK constraint on delete.

- [ ] **Step 4: Commit**

```bash
git add crates/oikonomia-core/tests && git commit -m "test: failing regressions for report filters (F1) and entity delete with documents (F3)"
```

---

### Task 2: Transactions + entity delete order (F2, F3)

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/journals.rs` (`post_entry`, `void_entry`)
- Modify: `crates/oikonomia-core/src/ledger/entities.rs` (`create_entity`, `delete_entity`)

**Interfaces:**
- Consumes: existing `PostJournal`, `get_entry`, `validate_lines_for_post`.
- Produces: `fn insert_posted_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView>` (private, no transaction — reused by `post_entry`, `void_entry`, and Task 7's `post_simple_entry` via `post_entry`). Public signatures unchanged: `post_entry(conn: &Connection, ...)` etc.

- [ ] **Step 1: Refactor `post_entry`** — move its current body into private `insert_posted_entry`; the public fn becomes:

```rust
pub fn post_entry(conn: &Connection, input: &PostJournal) -> Result<PostedEntryView> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|err| Error::Io(err.to_string()))?;
    let view = insert_posted_entry(&tx, input)?;
    tx.commit().map_err(|err| Error::Io(err.to_string()))?;
    Ok(view)
}
```

`unchecked_transaction` (rusqlite) works on `&Connection`; safety holds because `AppState` serializes all access behind a `Mutex`. Rollback on drop covers every early `?` return.

- [ ] **Step 2: Wrap `void_entry`** in one `unchecked_transaction` covering get + reverse-insert (call `insert_posted_entry(&tx, …)`, NOT `post_entry`, to avoid nested transactions) + both `voided_by` updates, then commit.

- [ ] **Step 3: Wrap `create_entity`** (entity insert + template account loop) in one transaction the same way.

- [ ] **Step 4: Fix and wrap `delete_entity`** — one transaction, delete order: null `voided_by_entry_id` → `journal_lines` → `documents` (now BEFORE entries) → `journal_entries` → `accounts` → `entities`. Drop the `let _ =` swallow on the documents delete; it is a real statement now.

- [ ] **Step 5: Run tests**

Run: `cargo test -p oikonomia-core`
Expected: `documents_flow` PASSES; `report_correctness` still fails (F1 is Task 3); all pre-existing tests green.

- [ ] **Step 6: Commit**

```bash
git commit -am "fix: wrap multi-statement ledger writes in transactions; delete documents before entries (F2, F3)"
```

---

### Task 3: Report SQL rewrite + dashboard consistency + date normalization (F1, F9, F10)

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/reports.rs`
- Modify: `crates/oikonomia-core/src/ledger/journals.rs` (`list_entries`, `account_register`)

**Interfaces:**
- Produces: private `fn account_activity_lines(conn, entity_id: EntityId, account_type: Option<AccountType>, from: Option<Date>, to: Date) -> Result<Vec<ReportLine>>` replacing both `trial_balance`'s inline SQL and `period_lines`/`as_of_lines`.

- [ ] **Step 1: Implement `account_activity_lines`** with entry predicates in an inner join inside a subquery:

```rust
let sql = format!(
    "
    SELECT a.code, a.name, a.account_type,
           COALESCE(t.debits, 0), COALESCE(t.credits, 0)
    FROM accounts a
    LEFT JOIN (
        SELECT jl.account_id,
               SUM(jl.debit_minor) AS debits,
               SUM(jl.credit_minor) AS credits
        FROM journal_lines jl
        JOIN journal_entries je ON je.id = jl.entry_id
        WHERE je.status = 'posted'
          AND {ACTIVE_ENTRY_PREDICATE}
          AND (?2 IS NULL OR je.entry_date >= ?2)
          AND je.entry_date <= ?3
    GROUP BY jl.account_id
    ) t ON t.account_id = a.id
    WHERE a.entity_id = ?1
      AND (?4 IS NULL OR a.account_type = ?4)
    ORDER BY a.sort_order, a.code
    "
);
// params: entity_id, from.map(format_date), format_date(to), account_type.map(account_type_str)
```

`trial_balance` → `account_activity_lines(conn, entity, None, None, as_of)` keeping its existing "skip all-zero lines" + totals loop. `period_lines` → `(…, Some(ty), Some(from), to)`. Delete `as_of_lines`'s 1970 hack — as-of is now `from: None`.

- [ ] **Step 2: Fix dashboard count (F9)** — alias the table and reuse the predicate:

```rust
let count_sql = format!(
    "
    SELECT COUNT(1) FROM journal_entries je
    WHERE je.entity_id = ?1 AND je.status = 'posted'
      AND {ACTIVE_ENTRY_PREDICATE}
      AND je.entry_date >= ?2 AND je.entry_date <= ?3
    "
);
```

Also correct the doc comment on `recent_entry_count` ("entries in period", not "up to 8").

- [ ] **Step 3: Normalize dates before binding (F10)** — in `list_entries`, `account_register`, and `dashboard_summary`, replace the parse-then-bind-raw pattern:

```rust
let from = from.map(parse_date).transpose()?.map(format_date);
let to = to.map(parse_date).transpose()?.map(format_date);
// bind from.as_deref(), to.as_deref()
```

(`dashboard_summary` parses to `Date` already — bind `format_date(from_d)` / `format_date(to_d)` in the count query.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p oikonomia-core`
Expected: ALL PASS, including all of `report_correctness`.

- [ ] **Step 5: Commit**

```bash
git commit -am "fix: report queries honor status/void/date filters; dashboard count excludes voids; normalize date params (F1, F9, F10)"
```

---

### Task 4: Invoice parser correctness details (F11)

**Files:**
- Modify: `crates/oikonomia-core/src/documents/invoice.rs` (`normalize`)
- Modify: `crates/oikonomia-core/src/documents/analyze.rs` (use `default_currency`)

**Interfaces:**
- Produces: private `fn replace_eur_token(text: &str) -> String` in invoice.rs; private `fn currency_exponent(code: &str) -> u32` in analyze.rs. `DocumentSuggestion` shape unchanged (amount stays 2-exponent minor units; non-2-exponent currencies get `amount_minor: None`).

- [ ] **Step 1: Word-boundary EUR replacement** — in `normalize`, replace `t.replace("EUR", "€").replace("eur", "€")` with `replace_eur_token(&t)`: scan chars; replace a case-insensitive `EUR` run with `€` only when the preceding and following chars are not `is_alphabetic()`. Test:

```rust
#[test]
fn eur_token_replacement_keeps_words() {
    assert_eq!(replace_eur_token("TOTAL 10 EUR"), "TOTAL 10 €");
    assert_eq!(replace_eur_token("EUROBANK EUROPE"), "EUROBANK EUROPE");
}
```

- [ ] **Step 2: Currency exponent guard** — in `analyze_document_bytes`, delete `let _ = default_currency;` and after `finalize_suggestion`:

```rust
if currency_exponent(default_currency) != 2 && suggestion.amount_minor.is_some() {
    suggestion.amount_minor = None;
    suggestion.notes = format!(
        "{} Amount detection assumes 2-decimal currencies; enter the {default_currency} amount manually.",
        suggestion.notes
    );
}
```

with `currency_exponent`: `"JPY" | "KRW" | "VND" | "CLP" | "ISK" => 0`, `"BHD" | "KWD" | "OMR" | "TND" | "JOD" | "IQD" | "LYD" => 3`, else 2. Unit test: JPY suggestion has `amount_minor == None`; EUR keeps its amount.

- [ ] **Step 3: Run + commit**

Run: `cargo test -p oikonomia-core` → PASS (all existing invoice fixtures must stay green).

```bash
git commit -am "fix: EUR token replacement no longer corrupts words; non-2-exponent currencies skip amount prefill (F11)"
```

---

### Task 5: Dependency vulnerability + drop-path hardening (F4, F7)

**Files:**
- Modify: `Cargo.toml` (workspace deps: `pdf-extract = "0.12"`)
- Modify: `crates/oikonomia-core/src/documents/store.rs` (extract shared validation)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`document_analyze_path`)

**Interfaces:**
- Produces: `pub fn validate_document_file(filename: &str, mime: &str, size_bytes: u64) -> Result<()>` in `documents/store.rs`, re-exported from `documents/mod.rs`; consumed by `save_document` and the Tauri command.

- [ ] **Step 1: Upgrade pdf-extract** — set workspace `pdf-extract = "0.12"`, `cargo build -p oikonomia-core`. If 0.12 renamed `extract_text_from_mem`, adapt the two call sites (`analyze.rs`, invoice live-fixture test) to the new name; the NGS live PDF fixture test is the acceptance gate for extraction behavior.

- [ ] **Step 2: Verify audit** — Run: `cargo audit`. Expected: RUSTSEC-2026-0187 gone; only the unmaintained-GTK3 warnings remain (Tauri-inherited, allowed).

- [ ] **Step 3: Shared validation** — in store.rs:

```rust
/// Validate a candidate document before its bytes are loaded or stored.
pub fn validate_document_file(filename: &str, mime: &str, size_bytes: u64) -> Result<()> {
    if size_bytes == 0 {
        return Err(Error::Validation("empty file".into()));
    }
    if size_bytes > MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::Validation(format!(
            "file too large (max {} MB)",
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }
    if filename.trim().is_empty() {
        return Err(Error::Validation("filename is required".into()));
    }
    if !is_allowed_mime(mime) {
        return Err(Error::Validation(
            "unsupported file type — use PDF, PNG, JPEG, WebP, or plain text".into(),
        ));
    }
    Ok(())
}
```

`save_document` calls it (after `resolve_mime`) instead of its inline checks.

- [ ] **Step 4: Pre-read check in `document_analyze_path`** — before `fs::read`:

```rust
let meta = std::fs::metadata(&path).map_err(|e| crate::error::CommandError {
    code: "io".into(),
    message: format!("could not read dropped file: {e}"),
})?;
let mime = oikonomia_core::documents::resolve_mime("", &filename);
oikonomia_core::documents::validate_document_file(&filename, &mime, meta.len())
    .map_err(crate::error::CommandError::from)?;
```

- [ ] **Step 5: Test + commit** — unit test `validate_document_file` (oversize u64, empty, bad mime); `cargo test -p oikonomia-core` PASS.

```bash
git commit -am "fix: upgrade pdf-extract past RUSTSEC-2026-0187; validate size/type before reading dropped files (F4, F7)"
```

---

### Task 6: Vault change-password (spec §6 gap)

**Files:**
- Modify: `crates/oikonomia-core/src/vault/store.rs`
- Test: `crates/oikonomia-core/tests/vault_change_password.rs`

**Interfaces:**
- Produces: `impl Vault { pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> }` — consumed by Task 8's `vault_change_password` command.

- [ ] **Step 1: Write failing test** (`#![expect(clippy::expect_used)]`):

```rust
#[test]
fn change_password_rekeys_vault() {
    let dir = tempdir().expect("tempdir");
    let mut vault = Vault::open_path(dir.path()).expect("open");
    vault.init("old password 12345").expect("init");
    vault.change_password("old password 12345", "new password 12345").expect("change");
    assert_eq!(vault.status(), VaultStatus::Unlocked);
    vault.lock();
    assert!(vault.unlock("old password 12345").is_err(), "old password must fail");
    vault.unlock("new password 12345").expect("new password unlocks");
}

#[test]
fn change_password_rejects_wrong_old_and_weak_new() {
    // wrong old -> Error::InvalidPassword; new shorter than 12 chars -> Error::Validation
}
```

Run: `cargo test -p oikonomia-core --test vault_change_password` → FAIL (method missing).

- [ ] **Step 2: Implement** in store.rs:

```rust
/// Re-encrypt the vault under a new master password (SQLCipher rekey).
///
/// Closes any open connection first, verifies the old password on a fresh
/// connection, writes the new header to a temp file, rekeys, then atomically
/// renames the header. On success the vault is left unlocked under the new key.
pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
    let header = self.header.as_ref().ok_or(Error::VaultUninitialized)?.clone();
    validate_password(new)?;

    // The rekey must not race our own open connection's page cache.
    self.conn = None;

    let old_key = crypto::derive_key(old, &header)?;
    let db_path = vault_db_path(&self.data_dir);
    let conn = open_sqlcipher(&db_path, &old_key)?; // wrong old password fails here

    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let new_header = VaultHeader::new_with_salt(&salt);
    let new_key = crypto::derive_key(new, &new_header)?;

    // Stage the new header BEFORE rekey so a crash between the two steps
    // leaves both keys recoverable (real header still matches the old key
    // until the atomic rename below).
    let header_path = vault_header_path(&self.data_dir);
    let tmp_path = header_path.with_extension("json.tmp");
    let header_json =
        serde_json::to_string_pretty(&new_header).map_err(|err| Error::Io(err.to_string()))?;
    fs::write(&tmp_path, header_json).map_err(|err| Error::Io(err.to_string()))?;

    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .map_err(|err| Error::Io(err.to_string()))?;
    let pragma_key = Zeroizing::new(crypto::key_to_sqlcipher_pragma(&new_key));
    conn.pragma_update(None, "rekey", pragma_key.as_str())
        .map_err(|err| Error::Crypto(err.to_string()))?;
    drop(conn);

    fs::rename(&tmp_path, &header_path).map_err(|err| Error::Io(err.to_string()))?;

    self.header = Some(new_header);
    self.conn = Some(open_sqlcipher(&db_path, &new_key)?);
    Ok(())
}
```

- [ ] **Step 3: Run + commit** — `cargo test -p oikonomia-core` PASS.

```bash
git commit -am "feat: master password change via SQLCipher rekey with atomic header swap"
```

---

### Task 7: Simple-entry builder in Rust (logic leak #1)

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/journals.rs`
- Modify: `crates/oikonomia-core/src/ledger/mod.rs` (re-exports)
- Test: `crates/oikonomia-core/tests/simple_entry.rs`

**Interfaces:**
- Produces (consumed by Task 8 command + Task 9 frontend):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleEntryKind { Expense, Income, Bill, Transfer }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleBillStatus { Paid, Unpaid, PayExisting }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostSimpleEntry {
    pub entity_id: EntityId,
    pub kind: SimpleEntryKind,
    pub bill_status: Option<SimpleBillStatus>,
    pub entry_date: String,
    pub description: String,
    pub reference: Option<String>,
    pub amount_minor: i64,
    pub category_account_id: Option<AccountId>,
    pub wallet_account_id: Option<AccountId>,
    pub payable_account_id: Option<AccountId>,
    pub from_account_id: Option<AccountId>,
    pub to_account_id: Option<AccountId>,
}

pub fn post_simple_entry(conn: &Connection, input: &PostSimpleEntry) -> Result<PostedEntryView>
```

- [ ] **Step 1: Failing tests** — expense posts Dr category / Cr wallet; income Dr wallet / Cr category; bill unpaid Dr category / Cr payable; bill pay-existing Dr payable / Cr wallet; transfer Dr to / Cr from; rejections: amount ≤ 0, category of wrong `AccountType` (e.g. income account passed for an expense), income wallet that is a liability, transfer with `from == to`, bill without `bill_status`.

- [ ] **Step 2: Implement** — resolve each required role with `get_account`, check `account_type` per role (expense/bill category: `Expense`; income category: `Income`; income wallet: `Asset`; other wallets: `Asset | Liability`; payable: `Liability`; transfer endpoints: `Asset | Liability`, distinct), map role omissions and mismatches to `Error::Validation` with the account code in the message, build the two `CreateJournalLine`s exactly as `TransactionsPage.buildLines` does today, and delegate to `post_entry` (which owns the transaction).

- [ ] **Step 3: Run + commit** — `cargo test -p oikonomia-core` PASS.

```bash
git commit -am "feat: build expense/income/bill/transfer journal lines in Rust so the UI carries no accounting rules"
```

---

### Task 8: Tauri shell — auto-lock watchdog, async analysis, new commands (F5, F8)

**Files:**
- Modify: `apps/desktop/src-tauri/src/state.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `Vault::change_password` (Task 6), `post_simple_entry` (Task 7).
- Produces: commands `vault_change_password { old, new }`, `entry_post_simple { input: PostSimpleEntry }`; Tauri event `"vault-locked"` (no payload) consumed by Task 9.

- [ ] **Step 1: Restructure `AppState`** for sharing across threads:

```rust
pub struct AppState {
    vault: Arc<Mutex<Vault>>,
    ocr_model_dir: PathBuf,
    /// Seconds since UNIX_EPOCH of the last command touching the vault.
    last_activity: Arc<AtomicU64>,
    /// Cached lock timeout; refreshed on unlock and on settings change.
    lock_timeout_secs: Arc<AtomicU64>,
}
```

`with_vault` first stores `now_secs()` into `last_activity`, then locks. Add `pub fn vault(&self) -> Arc<Mutex<Vault>>`, `pub fn watchdog_handles(&self) -> (Arc<Mutex<Vault>>, Arc<AtomicU64>, Arc<AtomicU64>)`, and `pub fn set_lock_timeout_cache(&self, secs: u64)`.

- [ ] **Step 2: Watchdog thread** in state.rs, spawned from `lib.rs` setup after `app.manage`:

```rust
/// Lock the vault from Rust when idle, regardless of webview state (F5).
pub fn spawn_auto_lock(
    app: tauri::AppHandle,
    vault: Arc<Mutex<Vault>>,
    last_activity: Arc<AtomicU64>,
    lock_timeout_secs: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let idle = now_secs().saturating_sub(last_activity.load(Ordering::Relaxed));
            if idle < lock_timeout_secs.load(Ordering::Relaxed) {
                continue;
            }
            let mut locked_now = false;
            if let Ok(mut vault) = vault.lock() {
                if vault.status() == VaultStatus::Unlocked {
                    vault.lock();
                    locked_now = true;
                }
            }
            if locked_now {
                use tauri::Emitter;
                let _ = app.emit("vault-locked", ());
            }
        }
    });
}
```

`vault_unlock` and `settings_set_lock_timeout` refresh the cache from `get_lock_timeout_secs` / the new value; `vault_unlock` also touches activity.

- [ ] **Step 3: Async analysis without holding the lock during OCR (F8)** — make `document_analyze` / `document_analyze_path` `async`; each clones `Arc<Mutex<Vault>>` + model dir, then `tauri::async_runtime::spawn_blocking` a three-phase helper: (1) lock → `get_entity` + `save_document` + `suggest_accounts_for_entity`, unlock; (2) `analyze_document_bytes` with no lock held; (3) lock → `save_analysis_json`. `.await` the join handle, mapping a join error to `CommandError { code: "io", … }`.

- [ ] **Step 4: New commands** — `vault_change_password(state, old: String, new: String) -> CommandResult<VaultStatus>` calling `vault.change_password`; `entry_post_simple(state, input: PostSimpleEntry) -> CommandResult<PostedEntryView>`. Register both plus keep the full handler list in `lib.rs`.

- [ ] **Step 5: Build + commit** — `cargo build -p oikonomia` (shell has no tests; core suite must stay green).

```bash
git commit -am "feat: Rust-side idle auto-lock with vault-locked event; async OCR analysis; change-password and simple-entry commands (F5, F8)"
```

---

### Task 9: Frontend — remove logic leaks, wire new commands (F6, F12, logic #2/#3)

**Files:**
- Modify: `web/src/lib/api.ts`, `web/src/lib/tauri.ts`
- Modify: `web/src/App.tsx`, `web/src/pages/TransactionsPage.tsx`, `web/src/pages/DashboardPage.tsx`, `web/src/pages/SettingsPage.tsx`

**Interfaces:**
- Consumes: commands from Task 8; event `"vault-locked"`.
- Produces: `api.entryPostSimple(input)`, `vaultChangePassword(old, new)` (in tauri.ts).

- [ ] **Step 1: Delete the description-prefix void filter (F6)** — in TransactionsPage `visibleEntries` and its `confirmVoid` optimistic filter, and in DashboardPage `activity`, filter on `!e.is_voided` only.

- [ ] **Step 2: Replace `buildLines` + `entryPost` with `entryPostSimple`** — `api.entryPostSimple` sends `{ input: { entity_id, kind, bill_status, entry_date, description, reference, amount_minor, category_account_id, wallet_account_id, payable_account_id, from_account_id, to_account_id } }` (nulls for unused roles; `bill_status` only for `kind === 'bill'`). Delete `buildLines`; keep `validateSelection` purely as a pre-flight UX message (backend re-validates). Keep `api.entryPost` exported for the raw command.

- [ ] **Step 3: Backend lock events + timeout propagation (F12)** — App.tsx: `listen<null>('vault-locked', () => { setStatus('locked'); setEntities([]); setEntityId(null); })` from `@tauri-apps/api/event` inside a `useEffect` with unlisten cleanup; pass `onLockTimeoutChange={setLockTimeoutSecs}` into SettingsPage and call it in `saveLock` (keep the JS idle timer as a fast-path duplicate of the Rust watchdog).

- [ ] **Step 4: Exponent-aware prefill** — in `applySuggestion`:

```ts
const digits = currencyFractionDigits(entity.base_currency)
if (s.amount_minor != null && s.amount_minor > 0 && digits === 2) {
  setAmount((s.amount_minor / 100).toFixed(2))
}
```

(import `currencyFractionDigits`; backend already nulls the amount for non-2-exponent currencies — this is defense in depth). `TransactionsPage` receives the digits via its existing `entity` prop.

- [ ] **Step 5: Change-password card** in SettingsPage — three password inputs (current, new, confirm; `minLength={12}`), client checks new === confirm, calls `vaultChangePassword(old, new)`, success clears fields and shows "Password changed" via the existing error-banner slot pattern (add a `notice` state rendered like ErrorBanner but with accent tokens).

- [ ] **Step 6: Verify + commit** — `cd web && npx tsc -b && npm run build` clean.

```bash
git commit -am "refactor: move entry construction and void filtering to backend results; backend-driven lock; change-password UI (F6, F12)"
```

---

### Task 10: Frontend unit tests (vitest)

**Files:**
- Modify: `web/package.json` (devDep `vitest`, script `"test": "vitest run"`)
- Create: `web/src/lib/money.test.ts`

- [ ] **Step 1: Install** — `cd web && npm install -D vitest`.

- [ ] **Step 2: Tests** for the two pure formatters the UI owns:

```ts
import { describe, expect, test } from 'vitest'
import { parseMajorToMinor, formatDate } from './money'

describe('parseMajorToMinor', () => {
  test('dot and comma decimals', () => {
    expect(parseMajorToMinor('25.50')).toBe(2550)
    expect(parseMajorToMinor('25,50')).toBe(2550)
  })
  test('thousand separators', () => {
    expect(parseMajorToMinor('1.234,56')).toBe(123456)
    expect(parseMajorToMinor('1,234.56')).toBe(123456)
  })
  test('rejects garbage and over-precise fractions', () => {
    expect(parseMajorToMinor('')).toBeNull()
    expect(parseMajorToMinor('abc')).toBeNull()
    expect(parseMajorToMinor('1.234', 'EUR')).toBeNull() // ambiguous 3-digit group
    expect(parseMajorToMinor('25.505', 'EUR')).toBeNull()
  })
  test('zero-decimal currency', () => {
    expect(parseMajorToMinor('1234', 'JPY')).toBe(1234)
    expect(parseMajorToMinor('12.34', 'JPY')).toBeNull()
  })
})

describe('formatDate', () => {
  test('passes ISO strings through', () => {
    expect(formatDate('2026-03-15')).toBe('2026-03-15')
  })
  test('normalizes {year, month, day} objects', () => {
    expect(formatDate({ year: 2026, month: 'March', day: 5 })).toBe('2026-03-05')
    expect(formatDate({ year: 2026, month: 3, day: 5 })).toBe('2026-03-05')
  })
})
```

Note: vitest runs in Node — `Intl` digit resolution works there. If any expectation disagrees with actual behavior, the implementation is the spec for pass-through cases; fix the test, not the formatter, unless the behavior is one reviewed as a bug.

- [ ] **Step 3: Run + commit** — `npm test` PASS.

```bash
git commit -am "test: vitest coverage for money parsing and date normalization"
```

---

### Task 11: Hygiene gate (fmt, clippy, lockfile, docs)

**Files:**
- Modify: whatever clippy/fmt touch; `Cargo.lock`; `README.md`

- [ ] **Step 1: Format** — `cargo fmt --all`; verify `cargo fmt --all -- --check` clean.

- [ ] **Step 2: Clippy to zero** — `cargo clippy --all-targets --all-features -- -D warnings` and fix every warning (baseline 64: collapsible `if`s → merged conditions; extension comparisons → a `has_extension(name, ext)` helper using `Path::extension` + `eq_ignore_ascii_case`; doc backticks; `f64`→`u32` casts in `ocr.rs` preprocess → clamp then `#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to image dimensions")]` on the two resize blocks; unnecessary raw-string hashes; single-char `str::contains` patterns → char literals; redundant closures; `map().unwrap_or()` → `map_or`). Never `#[allow]` (denied); use `#[expect(..., reason = "...")]`.

- [ ] **Step 3: Prune stale lockfile entries** — `cargo update --workspace` (rewrites the lock minimally and drops unreferenced `reqwest`/`hyper` trees); verify with `grep -c 'name = "reqwest"' Cargo.lock` → 0 and a full `cargo test -p oikonomia-core`.

- [ ] **Step 4: Docs** — README "Security notes": auto-lock is enforced in Rust (idle watchdog) and password change exists; v1 features list gains "change master password". AGENTS.md schema note: mention `post_simple_entry` as the UI entry path.

- [ ] **Step 5: Final gate** — run everything:

```bash
cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings \
  && cargo test -p oikonomia-core && cargo build -p oikonomia \
  && (cd web && npx tsc -b && npm test && npm run build) && cargo audit
```

Expected: all green; audit shows only unmaintained-GTK warnings.

- [ ] **Step 6: Commit**

```bash
git commit -am "chore: fmt + clippy clean at -D warnings; prune stale lockfile entries; document Rust-side auto-lock"
```
