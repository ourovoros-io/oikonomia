# 2026-08-13 Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix every bug and suggestion from the 2026-08-13 full-application review (`b92f17cb`): year-boundary balance-sheet math, invoice classify/parse panics, lock-event contract, vault open/create footgun, UI book isolation and double-submit, plus the listed hardening and performance items.

**Architecture:** Business-rule fixes stay in `oikonomia-core` (reports, invoice, vault, schema v5, list batching). The Tauri shell only gains lock-event emission, a read-only `vault_status`, a drop-path allowlist, and a prefs write mutex. The React UI cancels stale book loads, closes stale modals, and uses a synchronous `busyRef` on post.

**Tech Stack:** Rust 2024 / rusqlite 0.40 (SQLCipher) / Tauri 2 / React 19 + TypeScript / vitest.

## Global Constraints

- Money is integer minor units (`i64`); never `f64` for currency (AGENTS.md).
- Business rules live in `oikonomia-core`, not TypeScript.
- Workspace lints: `unwrap_used = deny`, `panic = deny`, `allow_attributes = deny` (use `#[expect(...)]`, never `#[allow(...)]`); test files that use `.expect()` start with `#![expect(clippy::expect_used)]`.
- `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings` must be clean at the end.
- `cargo test -p oikonomia-core` and `cd web && npm test` must pass at the end.
- No emojis in code, UI chrome, or commits. No `Co-Authored-By` lines.
- Commits are small, one per task, message explains why.
- No network permission may be added to Tauri capabilities; CSP stays `'self'` + IPC only.
- Idle auto-lock stays Rust-enforced (`spawn_auto_lock`).
- Existing vaults must keep opening: new Argon2 defaults apply only to newly written headers; schema v5 must migrate v4 vaults in a transaction.
- rustfmt: `use_small_heuristics = "Default"`. Call hippius-mem recall before edits; remember durable gotchas.

## File map

| Area | Files |
|------|--------|
| Balance sheet | `crates/oikonomia-core/src/ledger/reports.rs`, `crates/oikonomia-core/tests/report_correctness.rs` |
| Invoice | `crates/oikonomia-core/src/documents/invoice.rs` |
| Vault open/rekey | `crates/oikonomia-core/src/vault/store.rs`, `crates/oikonomia-core/src/vault/crypto.rs`, `crates/oikonomia-core/src/vault/header.rs`, `crates/oikonomia-core/tests/vault_change_password.rs` |
| Money / accounts | `crates/oikonomia-core/src/money.rs`, `crates/oikonomia-core/src/ledger/accounts.rs` |
| List perf + docs | `crates/oikonomia-core/src/ledger/journals.rs`, `crates/oikonomia-core/src/documents/store.rs`, `web/src/lib/api.ts`, `web/src/pages/DocumentsPage.tsx` |
| Schema v5 | `crates/oikonomia-core/src/db/schema.rs`, `crates/oikonomia-core/tests/migration_v4.rs` (or new `migration_v5.rs`) |
| Analyze/OCR | `crates/oikonomia-core/src/documents/ocr.rs`, `analyze.rs`, `pdf_repair.rs` |
| Tauri | `apps/desktop/src-tauri/src/{commands,state,lib}.rs`, `web/src/lib/api.ts`, `web/src/App.tsx`, `web/src/QuickAddApp.tsx` |
| Frontend isolation | `web/src/pages/{Reports,Accounts,Transactions,Documents,Dashboard,QuickAdd}Page.tsx`, `web/src/App.tsx` |

## Locked design choices

1. **Balance sheet:** keep “Net Income (current period)” for `[fy_start, as_of]`. Add “Retained Earnings (prior periods)” (code `RE`) for all unclosed P&L strictly before `fy_start`. Omit a line when its amount is 0. Do not write a permanent close journal.
2. **Income classify:** Income only from `is_sales_invoice` (client block + invoice label) or the exact phrase `sales invoice`. Drop `τιμολόγιο παροχ` / `παροχή υπηρεσι` / `ενδοκοινοτικ` / `invoice`+`service` as Income triggers. The existing SAMPLE_GREEK_INVOICE fixture stays Income because it has `Στοιχεία Πελάτη`.
3. **bill_unpaid:** arrears language only (`ληξιπρόθεσμ`, `ανεξόφλητ`, `amount due`, `unpaid`, `outstanding`, `please pay`, `επί πιστώσει`). Remove `εμπρόθεσμ` and `εκκαθαριστικ` as unpaid triggers.
4. **Unlock open flags:** `open_sqlcipher(path, key, create: bool)`. Init uses `create = true`. Unlock / change-password use `create = false` (`SQLITE_OPEN_READ_WRITE` only). Header present + missing/empty db → `VaultCorrupt`.
5. **KDF:** production defaults `m_cost = 65_536`, `t_cost = 3`, `p_cost = 1`. Accept window: m 8_192–1_048_576, t 1–8, p 1–4; outside → `VaultCorrupt` before hashing. `#[cfg(test)]` keeps the old weaker defaults so the suite stays fast.
6. **Scanned PDF OCR:** do **not** add pdfium. If PDF text extract is empty/below 8 chars, extract embedded image XObjects via `lopdf` and OCR those. If none, leave the existing empty-extract notes.
7. **Path allowlist:** Rust records canonicalized paths from `WindowEvent::DragDrop`. Path commands refuse anything not in that set. Frontend drop handlers that already go through Tauri drag-drop need no payload change.
8. **`vault_status` vs touch:** `vault_status` is read-only. New `vault_touch` command is the heartbeat. App.tsx idle heartbeat calls `vault_touch`. Quick-add lock probes stay on `vault_status`.

---

### Task 1: Balance sheet prior-period equity

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/reports.rs` (`balance_sheet`, doc comment on `BalanceSheet`)
- Test: `crates/oikonomia-core/tests/report_correctness.rs`

**Interfaces:**
- Consumes: `fiscal_year_start`, `sum_types_in_range`, `as_of_lines`
- Produces: equity line `{ code: "RE", name: "Retained Earnings (prior periods)", balance_minor: prior_net }` when `prior_net != 0`

- [ ] **Step 1: Write the failing test** in `report_correctness.rs`:

```rust
#[test]
fn balance_sheet_balances_after_fiscal_year_boundary() {
    let (_dir, vault) = setup_vault();
    let conn = vault.connection().expect("conn");
    let entity_id = setup_entity(conn);
    post_expense(conn, entity_id, "2026-06-01", 1_000);

    let bs_2026 = balance_sheet(conn, entity_id, "2026-12-31").expect("bs 2026");
    assert_eq!(bs_2026.total_assets, bs_2026.total_liabilities_equity);
    assert_eq!(bs_2026.total_assets, -1_000);

    let bs_2027 = balance_sheet(conn, entity_id, "2027-01-31").expect("bs 2027");
    assert_eq!(
        bs_2027.total_assets, bs_2027.total_liabilities_equity,
        "2027 as-of must still balance: assets {} vs L+E {}",
        bs_2027.total_assets, bs_2027.total_liabilities_equity
    );
    assert_eq!(bs_2027.total_assets, -1_000);
    assert!(
        bs_2027.equity.lines.iter().any(|l| l.code == "RE" && l.balance_minor == -1_000),
        "prior-year P&L must appear as RE: {:?}",
        bs_2027.equity.lines
    );
    assert!(
        !bs_2027.equity.lines.iter().any(|l| l.code == "NI"),
        "current-FY NI must be omitted when zero: {:?}",
        bs_2027.equity.lines
    );
}
```

Also add a July-FY case: `fiscal_year_start_month: Some(7)`, expense on `2026-03-15`, as-of `2026-08-01` must balance and carry March in `RE`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oikonomia-core --test report_correctness balance_sheet_balances_after_fiscal_year_boundary -- --nocapture`
Expected: FAIL — `total_assets != total_liabilities_equity` (−1000 vs 0)

- [ ] **Step 3: Implement**

In `balance_sheet`, after computing `fy_start` and current-FY `net`:

```rust
let prior_end = fy_start - time::Duration::days(1);
let prior_net = sum_types_in_range(conn, entity_id, &[AccountType::Income], Date::MIN, prior_end)?
    .saturating_sub(sum_types_in_range(
        conn,
        entity_id,
        &[AccountType::Expense],
        Date::MIN,
        prior_end,
    )?);
```

`Date::MIN` is wrong if `sum_types_in_range` rejects it — use `None` as the `from` bound (the existing `account_activity_lines` already accepts `from: Option<Date>`). Prefer extending `sum_types_in_range` with an optional from, or pass the earliest bound the helper already uses for as-of lines (`None` / unbounded start).

Push `RE` when `prior_net != 0`, then `NI` when `net != 0`. Update the `balance_sheet` doc comment: equity includes current-FY NI **and** unclosed prior-period P&L.

- [ ] **Step 4: Run tests**

Run: `cargo test -p oikonomia-core --test report_correctness --test ledger_flow`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/oikonomia-core/src/ledger/reports.rs crates/oikonomia-core/tests/report_correctness.rs
git commit -m "$(cat <<'EOF'
fix: fold prior-year P&L into balance-sheet equity

Current-FY NI alone left Assets != L+E after a year boundary
because there is no permanent close into retained earnings.
EOF
)"
```

---

### Task 2: Invoice classify, fullwidth colon, unpaid markers

**Files:**
- Modify: `crates/oikonomia-core/src/documents/invoice.rs` (`classify_kind`, `value_after_colon`, unpaid token lists)

**Interfaces:**
- `fn value_after_colon(line: &str) -> Option<String>` — advance by matched char `len_utf8()`
- `fn classify_kind(lower: &str) -> (EntryKindSuggestion, bool)` — Income only via `is_sales_invoice` or `"sales invoice"`

- [ ] **Step 1: Write failing tests** in the existing `invoice.rs` `#[cfg(test)]` module:

```rust
#[test]
fn received_service_invoice_is_not_income() {
    let text = "\
Τιμολόγιο Παροχής Υπηρεσιών\n\
Επωνυμία: ACME ΛΟΓΙΣΤΙΚΗ ΙΚΕ\n\
Α.Φ.Μ.: 123456789\n\
Πληρωτέο (€): 200,00\n";
    let s = parse_invoice_text(text);
    assert_ne!(s.kind, EntryKindSuggestion::Income, "kind={:?}", s.kind);
}

#[test]
fn value_after_fullwidth_colon_does_not_panic() {
    assert_eq!(value_after_colon("Name：ACME LTD"), Some("ACME LTD".into()));
    assert_eq!(value_after_colon("Name: ACME LTD"), Some("ACME LTD".into()));
}

#[test]
fn settlement_bill_is_not_automatically_unpaid() {
    let text = "\
ΔΕΗ\nΕΚΚΑΘΑΡΙΣΤΙΚΟΣ ΛΟΓΑΡΙΑΣΜΟΣ\n\
Πληρωμή εμπρόθεσμα έως 10/08/2026\n\
Συνολικό Ποσό Πληρωμής 50,00 €\n";
    let s = parse_invoice_text(text);
    assert!(!s.bill_unpaid, "εμπρόθεσμο/εκκαθαριστικό must not force unpaid");
}
```

Keep `greek_service_invoice_total_and_kind` asserting Income (issuer-side `Στοιχεία Πελάτη`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oikonomia-core received_service_invoice value_after_fullwidth settlement_bill -- --nocapture`
Expected: `received_service_invoice` FAIL (Income); `value_after_fullwidth` PANIC on byte boundary; `settlement_bill` FAIL (`bill_unpaid == true`)

- [ ] **Step 3: Implement**

`classify_kind` sales branch:

```rust
let sales = is_sales_invoice(lower) || lower.contains("sales invoice");
```

`value_after_colon`:

```rust
fn value_after_colon(line: &str) -> Option<String> {
    let (idx, ch) = line
        .char_indices()
        .find(|(_, c)| *c == ':' || *c == '：')?;
    let v = line[idx + ch.len_utf8()..].trim();
    if v.is_empty() { None } else { Some(v.to_owned()) }
}
```

Remove `εμπρόθεσμ` from the utility unpaid list and drop `unpaid || lower.contains("εκκαθαριστικ")` — return `unpaid` only.

- [ ] **Step 4: Run tests**

Run: `cargo test -p oikonomia-core --lib documents::invoice`
Expected: PASS, including `greek_service_invoice_total_and_kind`

- [ ] **Step 5: Commit**

```bash
git add crates/oikonomia-core/src/documents/invoice.rs
git commit -m "$(cat <<'EOF'
fix: stop treating received service invoices as income

Income now requires an issuer-side client block. Also slice
fullwidth colons on a char boundary and reserve unpaid for arrears.
EOF
)"
```

---

### Task 3: Money field private + custom Deserialize

**Files:**
- Modify: `crates/oikonomia-core/src/money.rs`
- Modify any `amount_minor` field access in Rust (must become `amount_minor()`)

**Interfaces:**
- `Money { amount_minor }` field is private
- JSON shape stays `{ "amount_minor": N }`
- Deserialize calls `from_minor`; negative → error

- [ ] **Step 1: Write failing tests** in `money.rs` `#[cfg(test)]`:

```rust
#[test]
fn deserialize_rejects_negative() {
    let err = serde_json::from_str::<Money>(r#"{"amount_minor":-1}"#);
    assert!(err.is_err());
}

#[test]
fn deserialize_accepts_zero_and_positive() {
    let z: Money = serde_json::from_str(r#"{"amount_minor":0}"#).expect("zero");
    assert_eq!(z.amount_minor(), 0);
    let p: Money = serde_json::from_str(r#"{"amount_minor":50}"#).expect("pos");
    assert_eq!(p.amount_minor(), 50);
}
```

- [ ] **Step 2: Run to verify fail** (after making field private the unit tests that construct `Money { amount_minor: … }` will also fail to compile — do the test first against current public field: deserialize of `-1` currently **succeeds**)

Run: `cargo test -p oikonomia-core --lib money::tests::deserialize_rejects_negative`
Expected: FAIL (Ok(Money { amount_minor: -1 }))

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Money {
    amount_minor: i64,
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw { amount_minor: i64 }
        let raw = Raw::deserialize(deserializer)?;
        Money::from_minor(raw.amount_minor).map_err(serde::de::Error::custom)
    }
}
```

Replace every in-crate `foo.amount_minor` field read with `foo.amount_minor()`. TS types stay `{ amount_minor: number }` (serialized form).

- [ ] **Step 4: Run**

Run: `cargo test -p oikonomia-core --lib money && cargo test -p oikonomia-core`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
fix: keep Money non-negative through Deserialize

The pub field and derived Deserialize let callers bypass from_minor.
EOF
)"
```

---

### Task 4: System accounts cannot be deactivated via `update_account`

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/accounts.rs`
- Test: `crates/oikonomia-core/tests/edit_and_opening_balance.rs` (or a small unit/integration test next to existing account tests)

- [ ] **Step 1: Failing test** — load Opening Balances (`is_system`), call `update_account` with `is_active: false`, expect `Error::Validation` containing `"system accounts cannot be archived"` (same string as `archive_account`).

- [ ] **Step 2: Run — expect FAIL** (update succeeds)

- [ ] **Step 3: In `update_account`, `get_account` first; if `account.is_system && !input.is_active` return that Validation error. Then run the existing UPDATE.

- [ ] **Step 4: `cargo test -p oikonomia-core --test edit_and_opening_balance`** — PASS

- [ ] **Step 5: Commit** `fix: refuse deactivating system accounts in update_account`

---

### Task 5: Vault open-without-create, init header order, change-password durability

**Files:**
- Modify: `crates/oikonomia-core/src/vault/store.rs`
- Modify: `crates/oikonomia-core/src/vault/crypto.rs` (`key_to_sqlcipher_pragma` returns `Zeroizing<String>`)
- Test: `crates/oikonomia-core/tests/vault_change_password.rs` and `store.rs` `#[cfg(test)]` if present

**Interfaces:**
- `fn open_sqlcipher(path: &Path, key: &VaultKey, create: bool) -> Result<Connection>`
- Init: write header (temp + rename) **before** creating the db; on later failure delete orphan db/wal/shm
- Unlock/change-password: `create = false`; missing/empty db + header → `VaultCorrupt`
- `change_password`: `sync_all` staged header + parent dir; read checkpoint `(blocked, log, checkpointed)` and abort if `blocked != 0`; keep `self.conn` until rekey succeeds (or reopen with `old_key` on error)

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn unlock_missing_db_is_corrupt_not_wrong_password() {
    let dir = TempDir::new().expect("dir");
    let mut vault = Vault::open_path(dir.path()).expect("open");
    vault.init(PW).expect("init");
    vault.lock();
    std::fs::remove_file(dir.path().join("vault.db")).expect("rm db");
    // leave header
    let err = Vault::open_path(dir.path())
        .expect("reopen")
        .unlock(PW)
        .expect_err("must not create empty db");
    assert!(matches!(err, Error::VaultCorrupt(_)), "{err:?}");
    assert!(!dir.path().join("vault.db").exists(), "must not plant a new ciphertext");
}

#[test]
fn derive_rejects_huge_m_cost() {
    let mut header = VaultHeader::new_with_salt(&[1u8; 16]);
    header.m_cost = 50_000_000;
    let err = derive_key("correct horse battery staple", &header).expect_err("bound");
    assert!(matches!(err, Error::VaultCorrupt(_)), "{err:?}");
}
```

(Second test is Task 6 — keep it there if you split. If implementing KDF bounds in this task, include it here and skip the duplicate in Task 6.)

- [ ] **Step 2: Run — unlock test FAIL** (`InvalidPassword` and/or `vault.db` recreated)

- [ ] **Step 3: Implement open flags, init order, checkpoint check, sync_all, restore conn on change-password error.** Use `rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE` without `CREATE` when `create == false`. If `!create && (!path.exists() || metadata.len() == 0)` return `VaultCorrupt` before `Connection::open`.

Checkpoint:

```rust
let blocked: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
if blocked != 0 {
    return Err(Error::Io("wal checkpoint blocked; will not rekey".into()));
}
```

- [ ] **Step 4: `cargo test -p oikonomia-core --test vault_change_password` plus the new tests — PASS**

- [ ] **Step 5: Commit** `fix: do not CREATE on unlock and harden password change`

---

### Task 6: Argon2 production defaults + param window

**Files:**
- Modify: `crates/oikonomia-core/src/vault/header.rs`, `crates/oikonomia-core/src/vault/crypto.rs`

- [ ] **Step 1: Test `derive_rejects_huge_m_cost`** (if not already in Task 5) plus `derive_rejects_zero_t_cost` (`t_cost = 0` → `VaultCorrupt`).

- [ ] **Step 2: FAIL** (huge m_cost currently attempts to hash / maps to `Crypto`)

- [ ] **Step 3:**

```rust
pub const DEFAULT_M_COST: u32 = 65_536;
pub const DEFAULT_T_COST: u32 = 3;

#[cfg(test)]
pub const DEFAULT_M_COST: u32 = 19_456;
#[cfg(test)]
pub const DEFAULT_T_COST: u32 = 2;
```

Cannot have two `pub const` with the same name — use:

```rust
#[cfg(not(test))]
pub const DEFAULT_M_COST: u32 = 65_536;
#[cfg(test)]
pub const DEFAULT_M_COST: u32 = 19_456;
```

Same for `t_cost`. In `derive_key`, before `Params::new`:

```rust
if !(8_192..=1_048_576).contains(&header.m_cost)
    || !(1..=8).contains(&header.t_cost)
    || !(1..=4).contains(&header.p_cost)
{
    return Err(Error::VaultCorrupt("argon2 parameters out of range".into()));
}
```

Return `Zeroizing<String>` from `key_to_sqlcipher_pragma`; update call sites that wrap again to just use the return value.

- [ ] **Step 4: `cargo test -p oikonomia-core --lib vault` — PASS**

- [ ] **Step 5: Commit** `fix: bound Argon2 params and raise production KDF defaults`

---

### Task 7: Batch `list_entries` + `document_list` description

**Files:**
- Modify: `crates/oikonomia-core/src/ledger/journals.rs`
- Modify: `crates/oikonomia-core/src/documents/store.rs` (`DocumentMeta`)
- Modify: `web/src/lib/api.ts` (`DocumentMeta.entry_description?: string | null`)
- Modify: `web/src/pages/DocumentsPage.tsx` (stop calling `entryList`)
- Test: `crates/oikonomia-core/tests/entry_filters.rs`, `documents_flow.rs`

**Interfaces:**
- `list_entries` computes `is_voided` in SQL:
  `je.voided_by_entry_id IS NOT NULL OR EXISTS (SELECT 1 FROM journal_entries x WHERE x.voided_by_entry_id = je.id)`
- Lines loaded in one query: `SELECT … FROM journal_lines WHERE entry_id IN (…)` then grouped in Rust
- `DocumentMeta.entry_description: String` from `JOIN journal_entries`

- [ ] **Step 1: Tests**
  - Existing list/filter tests must still pass (behavior-preserving).
  - `list_documents` returns `entry_description` equal to the linked entry’s description (`documents_flow.rs`).

- [ ] **Step 2: New description test FAIL** (field missing)

- [ ] **Step 3: Implement batching + JOIN.** Keep `PostedEntryView` shape. Empty IN-list → no line query.

- [ ] **Step 4: `cargo test -p oikonomia-core --test entry_filters --test documents_flow --test ledger_flow` and `cd web && npm test`**

- [ ] **Step 5: Commit** `perf: batch journal lines and return document entry descriptions`

---

### Task 8: `save_document` entity match + journal_lines XOR (schema v5)

**Files:**
- Modify: `crates/oikonomia-core/src/documents/store.rs`
- Modify: `crates/oikonomia-core/src/db/schema.rs` (`CURRENT_SCHEMA_VERSION = 5`, `migrate_v5`)
- Test: `crates/oikonomia-core/tests/documents_flow.rs`, new `tests/migration_v5.rs`

**Interfaces:**
- `save_document` loads the entry; if missing → `NotFound`; if `entry.entity_id != entity_id` → `AccountWrongEntity` or `Validation("document entry belongs to another book")`
- Constraint mapping: `ConstraintUnique` → duplicate name; other constraint codes → `Validation`/`Io` with the real reason
- v5: rebuild `journal_lines` with `CHECK ((debit_minor = 0) != (credit_minor = 0))` plus existing `>= 0` checks, inside one transaction that also bumps `schema_version` to 5 (same crash-safety as v4)

- [ ] **Step 1: Failing tests**
  - `save_document` with entry from entity B while passing entity A → Validation
  - After migrate to 5, `INSERT` a line with both debit and credit non-zero → SQLite constraint error

- [ ] **Step 2: FAIL** (save succeeds / insert succeeds)

- [ ] **Step 3: Implement.** Copy v4’s transactional rebuild pattern. Pre-check: if any existing line violates XOR, return `Io`/`VaultCorrupt` with a count (do not silently delete ledger data).

- [ ] **Step 4: `cargo test -p oikonomia-core --test documents_flow --test migration_v5 --test migration_v4`**

- [ ] **Step 5: Commit** `fix: require document/entry same book and XOR check on lines`

---

### Task 9: OCR poison recovery + PDF budgets + scanned-PDF image OCR

**Files:**
- Modify: `crates/oikonomia-core/src/documents/ocr.rs`
- Modify: `crates/oikonomia-core/src/documents/analyze.rs`
- Modify: `crates/oikonomia-core/src/documents/pdf_repair.rs`

**Interfaces:**
- `run_ocr_on_rgb`: on poison, `clear_poison`, set `ENGINE = None`, `ensure_engine` again. Wrap `get_text` / `detect_words` / `recognize_text` in `catch_unwind(AssertUnwindSafe(|| …))`; on panic reset `ENGINE` to `None` and return `Error::Analysis`.
- `pdf_text` / extract: abort if decompressed working set exceeds 32 MiB or page count exceeds 50.
- `repair_xref_offsets`: cap scans (e.g. max 256 stale xref entries or 8 MiB of search).
- `analyze_document_bytes`: if MIME is PDF and extracted text `chars().count() < 8`, try `extract_pdf_embedded_images(data)` (lopdf XObject) and `ocr_image_bytes` on the first decodable image. No new PDF rasterizer crate.

- [ ] **Step 1: Tests**
  - `value`/repair budget: a unit test that a tiny valid PDF still extracts.
  - Embedded-image helper: empty PDF → `None`; do not require OCR models in CI if models are absent (`ocr_available` false → skip OCR, still no panic).
  - Poison: call a test-only hook that poisons the mutex (or `std::panic::catch_unwind` around a helper) then assert the next `run_ocr` does not return `"OCR engine lock poisoned"` if models are present. If models are absent in unit tests, test the recovery function in isolation:

```rust
fn recover_engine_lock(guard: std::sync::LockResult<MutexGuard<Option<OcrEngine>>>) -> Result<MutexGuard<Option<OcrEngine>>> {
    match guard {
        Ok(g) => Ok(g),
        Err(poisoned) => {
            ENGINE.clear_poison();
            let mut g = poisoned.into_inner();
            *g = None;
            Ok(g)
        }
    }
}
```

- [ ] **Step 2–4: TDD the helper, then wire it.** `cargo test -p oikonomia-core --lib documents`

- [ ] **Step 5: Commit** `fix: recover OCR poison and cap PDF expansion`

---

### Task 10: Lock emit + split `vault_touch`

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `state.rs`, `lib.rs`
- Modify: `web/src/lib/api.ts`, `web/src/App.tsx`, `web/src/QuickAddApp.tsx`

**Interfaces:**
- `fn emit_vault_locked(app: &AppHandle)` used by `vault_lock` and `spawn_auto_lock`
- `vault_lock` takes `AppHandle` and emits after `vault.lock()`
- `vault_status` does **not** call `state.touch()`
- New `vault_touch` command: `state.touch(); Ok(())`
- App.tsx heartbeat: `vaultTouch()` instead of `vaultStatus()`
- Watchdog still emits only when **it** transitions, **and** the command emits when the user/UI locks — duplicate emits are fine (UI is idempotent)

- [ ] **Step 1:** No cheap Rust unit test for Tauri emit. Add a small `emit` helper and a frontend test is optional. Verify by reading both call sites. If `web/src/lib` grows `vaultTouch`, add nothing unless there is an existing api test.

- [ ] **Step 2: Implement + `cargo check -p oikonomia --offline` (or workspace clippy on the tauri crate)**

- [ ] **Step 3: Commit** `fix: emit vault-locked from every lock path`

---

### Task 11: Drop-path allowlist + prefs write mutex

**Files:**
- Modify: `apps/desktop/src-tauri/src/state.rs`, `lib.rs`, `commands.rs`

**Interfaces:**
- `AppState.allowed_drop_paths: Mutex<HashSet<PathBuf>>`
- `AppState.prefs_lock: Mutex<()>`
- `on_window_event`: on `WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. })`, canonicalize each path and insert
- `document_analyze_path` / `entry_post_simple_with_document_path`: canonicalize, require membership, then existing stat/MIME/size gates, **then** vault
- `settings_set_theme` / `settings_remember_quick_add`: hold `prefs_lock` across load-mutate-save

- [ ] **Step 1:** Core cannot easily simulate Tauri events. Extract `fn path_is_allowed(allowed: &HashSet<PathBuf>, path: &Path) -> bool` that compares canonicalized paths; unit-test it in `state.rs` or a small `paths.rs` module (allow symlink-resolved equality; reject `../` escapes).

- [ ] **Step 2–4: TDD the helper, wire commands.**

- [ ] **Step 5: Commit** `fix: accept drop paths only from native drag-drop`

---

### Task 12: Frontend book isolation + double-submit

**Files:**
- Modify: `web/src/App.tsx` (optional: `key={entity?.id}` on page hosts)
- Modify: `web/src/pages/ReportsPage.tsx`, `AccountsPage.tsx`, `TransactionsPage.tsx`, `DocumentsPage.tsx`, `DashboardPage.tsx`, `QuickAddPage.tsx`
- Test: add `web/src/lib/loadGeneration.test.ts` for a tiny helper if you extract one; otherwise test `busyRef` behavior via extracted `beginSubmit(busyRef): boolean`

**Interfaces:**
- Extract `web/src/lib/guards.ts`:

```ts
export function beginExclusive(busyRef: { current: boolean }): boolean {
  if (busyRef.current) return false
  busyRef.current = true
  return true
}
```

- Each page: on `entity.id` change, clear that page’s ledger state immediately; use a monotonic `generation` (or `cancelled` flag) and ignore stale responses.
- Accounts: `setBalanceAccount(null)` on entity change.
- Transactions: close form (`setShowForm(false)`, `setEditId(null)`), reset role ids; `onPost` starts with `if (!beginExclusive(busyRef)) return`.
- QuickAdd: `selectEntity` / `loadAccountsFor` ignore stale generations; `onSubmit` uses `beginExclusive(busyRef)` **before** validation awaits.

- [ ] **Step 1: Vitest for `beginExclusive`** (second call returns false without clearing).

- [ ] **Step 2: FAIL then implement helper.**

- [ ] **Step 3: Wire pages.** Simplest correct approach: in `App.tsx` render pages as `<ReportsPage key={entity?.id ?? 'none'} entity={entity} />` (and the same for the other book-scoped pages). That remounts and clears state. Still add `busyRef` + `beginExclusive` on post handlers because remount does not prevent double-click on the same book.

- [ ] **Step 4: `cd web && npm test && npm run build`**

- [ ] **Step 5: Commit** `fix: isolate book UI and prevent double post`

---

### Task 13: Format, clippy, full test gate

**Files:** workspace

- [ ] **Step 1:**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p oikonomia-core
cd web && npm test && npm run build
```

- [ ] **Step 2:** Fix any fallout. Do not weaken lints.

- [ ] **Step 3: Commit only if the gate produced extra diffs** `chore: fmt after review fixes`

---

## Spec coverage (self-review)

| Review issue | Task |
|---|---|
| Bug 1 BS year boundary | 1 |
| Bug 2 fullwidth colon panic | 2 |
| Bug 3 Income prefill | 2 |
| Bug 4 vault-locked emit | 10 |
| Bug 5 CREATE on unlock | 5 |
| Bug 6 stale book loads | 12 |
| Bug 7 set-balance other book | 12 |
| Bug 8 double-submit | 12 |
| Sugg list_entries N+1 | 7 |
| Sugg KDF defaults | 6 |
| Sugg unbounded KDF | 6 |
| Sugg change-password durability / checkpoint / restore conn | 5 |
| Sugg system account update | 4 |
| Sugg Money pub field | 3 |
| Sugg OCR poison | 9 |
| Sugg PDF zip-bomb | 9 |
| Sugg scanned PDF OCR | 9 |
| Sugg bill_unpaid | 2 |
| Sugg save_document cross-entity | 8 |
| Sugg path file-read oracle | 11 |
| Sugg prefs race | 11 |
| Sugg quick-add account race | 12 |
| Sugg transactions form on switch | 12 |
| Sugg Documents full entryList | 7 |
| Sugg journal_lines CHECK | 8 |
| Sugg init header-after-db | 5 |
| Nit vault_status heartbeat | 10 |
| Nit pragma hex zeroize | 5/6 |

No placeholders. Nits are folded into the related tasks.
