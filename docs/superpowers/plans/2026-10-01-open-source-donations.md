# Open Source and Donations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the paid licensing subsystem, relicense the repository under GPL-3.0-or-later, and add static crypto donation addresses to the app and README.

**Architecture:** Three pull requests. PR 1 removes licensing from the outside in (web, then desktop shell, then core) so every commit builds. PR 2 swaps the proprietary licence metadata and docs for open-source ones and audits history. PR 3 adds one Rust constant table of donation addresses, served to a new Settings section by a single IPC command.

**Tech Stack:** Rust 1.94 (workspace: `oikonomia-core`, `oikonomia-update`, `macos-dock-icon`, Tauri 2 desktop crate), React 19 + Vite + vitest, cargo-deny, gitleaks.

**Spec:** `docs/superpowers/specs/2026-10-01-open-source-donations-design.md`

## Global Constraints

- Licence identifier everywhere: `GPL-3.0-or-later`.
- All business logic lives in Rust. The UI renders what Rust returns and holds no rules.
- No new network access. `scripts/assert-core-offline.sh` and `cargo deny check` must keep passing.
- No new dependencies in any task.
- No emojis in code, UI, docs, or commits. No `Co-Authored-By` trailer on commits.
- Function names are full words; `pub` and `pub(crate)` items carry rustdoc.
- Rust gate before every commit that touches Rust: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the tests named in the task.
- Web gate before every commit that touches `web/`: `cd web && npx tsc -b && npm run lint && npm test`.
- The four locale files must keep identical key sets. `en.json` is flat (`"a.b.c": "..."`); `el.json`, `fr.json`, `de.json` are nested objects.
- Leftover `license.lic` files, the macOS Keychain item `trial-started-at`, and the `trial_started_at` pref on existing machines are ignored: never read, never deleted.
- Coins: BTC, ETH, XMR, DASH, LTC, SOL. USDC and USDT are accepted on the ETH and SOL addresses. Six addresses in total.

## Review Focus

1. A prefs file written by the paid build still contains `trial_started_at`; the app must load it and keep the other prefs. Pinned in Task 3.
2. A vault that was on an expired trial, with one entity: creating a second entity and posting an entry must now succeed. Pinned in Task 2 (shell no longer gates) and Task 3 (core has no gate).
3. A donation address pasted with a trailing newline or space, or with the wrong coin's format in the wrong row, must fail the build, not ship. Pinned in Task 7.
4. Clipboard write refused by the webview: the user must see a failure message and still be able to select the address by hand. Pinned in Task 8.
5. The marketing capture fixture answers every command Settings calls on mount; an unanswered command breaks the capture. Pinned in Tasks 1 and 8.

## File Structure

Deleted:

- `crates/oikonomia-core/src/license.rs`, `crates/oikonomia-core/tests/license_flow.rs`
- `crates/oikonomia-mint/` (whole crate)
- `apps/desktop/src-tauri/src/trial_store.rs`
- `web/src/components/TrialBanner.tsx`, `TrialBanner.test.tsx`
- `web/src/lib/license.ts`, `license.test.ts`
- `EULA.md`, `docs/commerce.md`

Created:

- `LICENSE` (GPL-3.0 text)
- `CONTRIBUTING.md`, `SECURITY.md`, `.github/FUNDING.yml`
- `apps/desktop/src-tauri/src/donations.rs`: the address table, its command, its tests
- `web/src/components/DonationAddresses.tsx` and its test: renders rows and the copy button

Modified: listed per task.

---

# PR 1: remove licensing

Branch: `feat/remove-licensing` from `main`.

### Task 1: Web stops using licensing

After this task the UI never calls `license_status`, `license_install`, or `eula_text`, shows no trial or license UI, and never disables "New entity". The Rust commands still exist; they are simply unused. The three `license_*` error codes stay mapped until Task 3, because the desktop crate still emits them and a Rust test pins `errorCodes.json`.

**Files:**
- Delete: `web/src/components/TrialBanner.tsx`, `web/src/components/TrialBanner.test.tsx`, `web/src/lib/license.ts`, `web/src/lib/license.test.ts`
- Modify: `web/src/App.tsx`, `web/src/App.test.tsx`, `web/src/pages/SettingsPage.tsx`, `web/src/pages/SettingsPage.test.tsx`, `web/src/pages/SettingsPage.i18n.test.tsx`, `web/src/lib/api.ts`, `web/src/lib/i18n.test.ts`, `web/src/locales/{en,el,fr,de}.json`, `web/marketing/fixture/handler.ts`, `web/marketing/fixture/handler.test.ts`, `web/package.json`, `web/package-lock.json`

**Interfaces:**
- Produces: `SettingsPage` no longer accepts `onLicenseChanged`. `api` no longer has `licenseStatus`, `licenseInstall`, `eulaText`. No export named `LicenseStatus` or `LicenseState` exists.

- [ ] **Step 1: Write the failing tests**

In `web/src/pages/SettingsPage.test.tsx`, rename `describe('SettingsPage license', ...)` to `describe('SettingsPage', ...)` and add these two tests at the top of it:

```tsx
  test('Settings has no License section and no trial copy', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const titles = screen.getAllByRole('heading', { level: 3 }).map((el) => el.textContent)
    expect(titles[0]).toBe('Language')
    expect(titles).not.toContain('License')
    expect(screen.queryByText(/trial/i)).toBeNull()
    expect(screen.queryByRole('button', { name: /import license/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /license agreement/i })).toBeNull()
  })

  test('New entity is enabled when the vault already has a book', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    const newEntity = await screen.findByRole('button', { name: /new entity/i })
    expect(newEntity).toBeEnabled()
    expect(screen.queryByText(/keep more than one book/i)).toBeNull()
  })
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd web && npx vitest run src/pages/SettingsPage.test.tsx -t "no License section"` and `-t "New entity is enabled"`
Expected: both FAIL (License heading present; New entity disabled under the mocked trial status).

- [ ] **Step 3: Remove the license UI from `SettingsPage.tsx`**

- Delete the import block from `'../lib/license'` (lines 23-30) and the `openUrl` import from `@tauri-apps/plugin-opener` (line 20). Remove `Key` from the `lucide-react` import.
- Delete the `onLicenseChanged` prop from the props type (with its doc comment) and from the destructuring.
- Delete the state hooks `license`, `licenseError`, `licenseBusy`, `eulaText`, `eulaOpen`.
- In the mount `useEffect`, delete the `api.licenseStatus()` and `api.eulaText()` chains and the long comment about `onLicenseChanged`, leaving:

```tsx
  useEffect(() => {
    void api
      .getLockTimeout()
      .then((secs) => setLockMins(Math.max(1, Math.round(secs / 60))))
      .catch(() => {
        /* ignore */
      })
  }, [])
```

  Remove the `eslint-disable-next-line react-hooks/exhaustive-deps` line only if it sat with the deleted comment; keep the empty dependency array.
- Delete `applyExpiredFromWrite`. Replace `commandErrorMessage` with:

```tsx
  function commandErrorMessage(err: unknown, fallback = ''): string | null {
    const cmd = err as CommandError
    return cmd.message || fallback
  }
```

- Delete `const addAnotherBook = ...`. In `onCreate`, delete the line `if (!canAddAnotherBook(license, entities.length)) return`.
- In the change-password `catch`, delete the `isLicenseExpiredCode` block and the `licenseCopy` block, so the catch goes straight from `const cmd = err as CommandError` to `setPageError(...)`.
- Delete `onImportLicense`.
- Delete the whole `<CollapsibleSection title={t('settings.license.title')} ...>` element (through its closing tag, just before the Support section).
- Delete the EULA `<Modal open={eulaOpen} ...>` element.
- In the Entities section footer, replace the block that renders the limit hint and the button with:

```tsx
        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--color-border)] px-5 py-3">
          <Button size="sm" className="ml-auto" onClick={() => setShowCreate(true)}>
            <Plus className="size-3.5" />
            {t('settings.entities.new')}
          </Button>
        </div>
```

- [ ] **Step 4: Remove licensing from `App.tsx`, `api.ts`, and delete the license files**

- `App.tsx`: delete the `TrialBanner` import, the `LicenseStatus` type import, the `license` state, the `try { setLicense(await api.licenseStatus()) } catch { ... }` block, the `<TrialBanner license={license} />` element, and the `onLicenseChanged={setLicense}` prop.
- `api.ts`: delete `import type { LicenseStatus } from './license'`, the `export type { LicenseState, LicenseStatus }` line, and the `licenseStatus`, `licenseInstall`, `eulaText` entries with their doc comments.
- `git rm web/src/components/TrialBanner.tsx web/src/components/TrialBanner.test.tsx web/src/lib/license.ts web/src/lib/license.test.ts`
- `web/marketing/fixture/handler.ts`: delete the `LicenseStatus` import, the `const license` line, and the `eula_text` and `license_status` answers. `handler.test.ts`: delete the `license_status` expectation on line 12.
- `@tauri-apps/plugin-opener` is now unused in `web/`: run `cd web && npm uninstall @tauri-apps/plugin-opener`.

- [ ] **Step 5: Remove the locale strings**

Delete these keys from all four locale files (flat keys in `en.json`; the matching nested objects in `el`/`fr`/`de`, removing an object entirely once it is empty):

```
settings.license.title            settings.license.description
settings.license.import           settings.license.buy
settings.license.licensedUntil    settings.license.replace
settings.license.banner.expired   settings.license.error.invalid
settings.license.error.signature  settings.license.error.wrongProduct
settings.license.error.unreadable settings.license.error.generic
settings.license.viewEula         settings.license.eulaTitle
settings.trial.banner.active      settings.trial.banner.expired
license.entityLimit               license.entityLimitHint
trial.banner.expiring             trial.banner.expired
```

Do not touch `error.licenseInvalid`, `error.licenseExpired`, `error.licenseEntityLimit` (Task 3), `rpt.trialBalance`, or `reports.trial.*`.

Change `settings.support.description` so it no longer mentions licences:

| File | New value |
|------|-----------|
| `en.json` | `Questions and bug reports: {email}` |
| `el.json` | `Ερωτήσεις και αναφορές σφαλμάτων: {email}` |
| `fr.json` | `Questions et rapports de bogue : {email}` |
| `de.json` | `Fragen und Fehlerberichte: {email}` |

- [ ] **Step 6: Update the remaining tests**

- `SettingsPage.test.tsx`: in the `vi.mock('../lib/api', ...)` factory and in `beforeEach`, delete the `licenseStatus`, `licenseInstall`, `eulaText` lines; delete the `vi.mock('@tauri-apps/plugin-opener', ...)` block, the `openUrl` import, and `vi.mocked(openUrl).mockReset()`. Delete every test whose subject is licensing. By title, those are:
  - `Language is first; License is present without a Buy button or extra trial helper`
  - `trial banner uses days remaining from license_status`
  - `licensed pill formats the date and offers Import another file`
  - `import license_invalid shows Writer generic, not Rust Display`
  - `import license_expired becomes the expired banner, not an import error`
  - `cancelled license_install leaves status unchanged`
  - `clicking License agreement shows the bundled EULA text`
  - `write-gated command surfaces license_expired as expired banner, not a crash`
  - `trial with one entity disables add-book and does not open create`
  - `licensed with one entity keeps add-book enabled and create can proceed`
  - `expired first-book create maps license_expired, not entityLimit`
  - the three `Buy a license ...` tests
  - `entity_create license_entity_limit shows Writer copy, not Rust Display`
  - the test ending with the `Import a signed license to keep more than one book` assertion (around line 690-706)
  - `licensed vault enables New entity and drops the limit hint`
  - `trial pill and Import license share one centered row`

  In any surviving test, delete a leftover `vi.mocked(api.licenseStatus)...` line. Then run `grep -niE 'licen|trial|buy|eula' web/src/pages/SettingsPage.test.tsx`; the only hits allowed are inside the two tests from Step 1.
- `SettingsPage.i18n.test.tsx`: delete the three mock lines (21-23).
- `App.test.tsx`: delete the two mock lines (42-43) and the whole `describe('App trial banner survives Settings visits', ...)` block.
- `i18n.test.ts`: in `sampleKeys` replace `'settings.license.title'` with `'settings.support.title'`; delete the assertions on `settings.license.*`, `settings.trial.*`, and `license.entityLimit*` (around lines 126-152 and 179-185); delete the whole `describe('Writer license catalog', ...)` block.

- [ ] **Step 7: Run the web gate**

Run: `cd web && npx tsc -b && npm run lint && npm test && npm run build`
Expected: all pass. The two tests from Step 1 now PASS. `git grep -niE 'licen[sc]e|TrialBanner|eula' -- web/src web/marketing` shows only `error.license*` keys, `commandError.ts`, `errorCodes.json`, and font licence files.

- [ ] **Step 8: Commit**

```bash
git add -A web
git commit -m "refactor(web): remove license, trial, and EULA UI

Oikonomia is becoming free and open source, so nothing in the UI may
gate on a license. The shell commands and error codes are still present
and are removed in the following commits."
```

### Task 2: Desktop shell stops gating

**Files:**
- Delete: `apps/desktop/src-tauri/src/trial_store.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `lib.rs`, `tray.rs`, `update.rs`, `update_key.rs`, `config_checks.rs`, `Cargo.toml`, `capabilities/default.json`

**Interfaces:**
- Consumes: `oikonomia_core::ledger::create_entity(conn: &Connection, input: &CreateEntity) -> Result<Entity>` (already public).
- Produces: no command named `license_status`, `license_install`, or `eula_text`. One vault helper, `with_vault_blocking`. The webview has no opener permission.

- [ ] **Step 1: Write the failing tests**

Append to `apps/desktop/src-tauri/src/config_checks.rs`, replacing `opener_allow_globs`, `opener_capability_is_scoped_to_the_buy_page_only`, and `support_mail_is_opened_from_rust_not_through_a_webview_glob`:

```rust
#[test]
fn webview_has_no_opener_permission() {
    // Nothing in the UI opens a URL any more. The support mailto is built
    // and opened from Rust (`open_support_email`), whose plugin API is not
    // capability-scoped, so the webview needs no opener grant at all.
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capabilities json");

    let opener: Vec<String> = capabilities["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .filter_map(|permission| {
            let identifier = permission
                .as_str()
                .or_else(|| permission["identifier"].as_str())?;
            identifier.starts_with("opener:").then(|| identifier.to_owned())
        })
        .collect();

    assert!(opener.is_empty(), "webview opener permissions: {opener:?}");
}

#[test]
fn no_licensing_commands_are_registered() {
    let registrations = include_str!("lib.rs");

    for command in ["license_status", "license_install", "eula_text"] {
        assert!(
            !registrations.contains(command),
            "{command} is still registered"
        );
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p oikonomia config_checks`
Expected: `webview_has_no_opener_permission` and `no_licensing_commands_are_registered` FAIL.

- [ ] **Step 3: Remove the commands and the gate from `commands.rs`**

- Delete the `use oikonomia_core::license::{...}` import. In the `oikonomia_core::ledger` import, replace `create_entity_allowed` with `create_entity`.
- Change the `SUPPORT_EMAIL` doc comment to: `/// Where users send questions and bug reports. Every support pointer the app shows derives from this one address.`
- In `vault_init` and `vault_unlock`, delete the `stamp_trial_start(&state)?;` line.
- Delete `stamp_trial_start` and `stamp_trial_start_with`.
- In the `mod tests` that follows them, delete `MemoryTrialStore`, its `impl`, the tests `vault_init_stamps_trial_without_unlock` and the old-trial test after it, and the now-unused imports (`stamp_trial_start_with`, the `oikonomia_core::license::{...}` import, `RefCell`, `load_ui_prefs` if nothing else in the module uses it). Keep `require_granted_path` tests and the `temp_dir` helper if they still have users; delete `temp_dir` if not.
- Delete `BUY_URL`, `LicenseStatusPayload`, `license_status`, `license_install`, and `mod license_payload_tests`.
- Replace `entity_create` with:

```rust
/// Create entity with chart template.
#[tauri::command]
pub async fn entity_create(
    state: State<'_, AppState>,
    input: CreateEntity,
) -> CommandResult<Entity> {
    with_vault_blocking(&state, move |vault| {
        let conn = vault.connection()?;
        create_entity(conn, &input)
    })
    .await
}
```

- Delete `require_writes` and `with_vault_write_blocking`. Rename every remaining call:

```bash
sed -i '' 's/with_vault_write_blocking/with_vault_blocking/g' apps/desktop/src-tauri/src/commands.rs
```

  Then `grep -c with_vault_write_blocking apps/desktop/src-tauri/src/*.rs` must print 0 for every file.
- Delete the `// --- Legal ---` section: `eula_text_content`, `eula_text`, `mod eula_tests`.

- [ ] **Step 4: Remove the supporting pieces**

- `lib.rs`: delete `mod trial_store;`, the registrations `commands::license_status`, `commands::license_install`, `commands::eula_text`, and change the comment above `tauri_plugin_opener::init()` to `// Used from Rust only (open_support_email); the webview holds no opener permission.`
- `git rm apps/desktop/src-tauri/src/trial_store.rs`
- `Cargo.toml`: delete the three-line comment and the `[target.'cfg(target_os = "macos")'.dependencies]` table with `security-framework = "3"`. First confirm nothing else uses it: `grep -rn security_framework apps crates` must show no hits outside the deleted file.
- `tray.rs`: delete `license_filter_label` and its four assertions in the test.
- `update.rs`: delete `use oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX;` and replace the first test with:

```rust
    #[test]
    fn baked_key_is_a_nonempty_minisign_key() {
        assert!(UPDATER_PUBLIC_KEY.len() > 32);
        parse_public_key(UPDATER_PUBLIC_KEY).expect("ops minisign public key must decode");
    }
```

- `update_key.rs`: remove the doc sentence that contrasts the key with `oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX`.
- `capabilities/default.json`: delete the `opener:allow-open-url` object (and the comma before it), and change `description` to `"Minimal local-only capabilities for Oikonomia (no network). File I/O stays in Rust: native dialogs pick a path, and the vault lives in the app data dir. No fs or opener permission is exposed to the webview."`

- [ ] **Step 5: Run the Rust gate**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p oikonomia
```
Expected: clippy clean; all tests pass, including the two from Step 1. `error.rs` still maps the three `License*` variants; that is correct until Task 3.

- [ ] **Step 6: Verify in the running app**

Run: `cargo tauri dev`. Unlock a vault, open Settings: no License section. Create a second entity and post one entry in it. Click "Email support" in Settings and confirm the mail client opens (proves the Rust-side opener works without the webview permission).

- [ ] **Step 7: Commit**

```bash
git add -A apps/desktop
git commit -m "refactor(desktop): remove license commands, trial stamp, and write gate

No command consults a license any more: every vault write goes through
with_vault_blocking and entity creation is uncapped. The webview opener
permission existed only for the buy page, so it is dropped rather than
re-pointed; the support mailto is opened from Rust."
```

### Task 3: Core has no licensing; mint crate and error codes removed

**Files:**
- Delete: `crates/oikonomia-core/src/license.rs`, `crates/oikonomia-core/tests/license_flow.rs`, `crates/oikonomia-mint/`
- Modify: `crates/oikonomia-core/src/lib.rs`, `src/error.rs`, `src/prefs.rs`, `src/ledger/entities.rs`, `src/ledger/mod.rs`, `crates/oikonomia-core/Cargo.toml`, `crates/oikonomia-core/tests/ledger_flow.rs`, `Cargo.toml`, `Cargo.lock`, `apps/desktop/src-tauri/src/error.rs`, `web/src/lib/errorCodes.json`, `web/src/lib/commandError.ts`, `web/src/locales/{en,el,fr,de}.json`

**Interfaces:**
- Produces: `oikonomia_core::error::Error` has no `License*` variant. `UiPrefs` has exactly `locale`, `last_entity_id`, `last_accounts_by_entity_kind`. `oikonomia_core::ledger` exports `create_entity` and `count_entities` but not `create_entity_allowed`.

- [ ] **Step 1: Write the regression tests**

These pin behaviour that must survive the deletion. They pass before and after; their job is to fail if a later change reintroduces a cap or breaks old prefs files.

Add to `crates/oikonomia-core/src/prefs.rs` tests, replacing `missing_trial_started_at_defaults_to_none`:

```rust
    #[test]
    fn prefs_file_from_the_paid_build_still_loads() {
        let Ok(dir) = tempdir() else {
            return;
        };

        // Written by builds that had a trial; the key is now unknown.
        let json = r#"{
            "locale": "de",
            "last_entity_id": "ent-1",
            "trial_started_at": "2026-09-01T10:00:00Z"
        }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());

        let prefs = load_ui_prefs(dir.path());
        assert_eq!(prefs.locale, Locale::De);
        assert_eq!(prefs.last_entity_id.as_deref(), Some("ent-1"));
    }
```

Add to `crates/oikonomia-core/tests/ledger_flow.rs` (it already imports `ChartTemplate`, `CreateEntity`, `create_entity`, `Vault`, and `tempdir`; add `count_entities` to the `oikonomia_core::ledger` import):

```rust
#[test]
fn a_vault_holds_any_number_of_entities() {
    let dir = tempdir().expect("temp dir");
    let mut vault = Vault::open_path(dir.path()).expect("open vault");
    vault.init("correct horse battery staple").expect("init vault");
    let conn = vault.connection().expect("connection");

    for name in ["Personal", "Company", "Side project"] {
        let created = create_entity(
            conn,
            &CreateEntity {
                name: name.into(),
                base_currency: "EUR".into(),
                chart_template: ChartTemplate::Personal,
                fiscal_year_start_month: None,
            },
        );
        assert!(created.is_ok(), "creating {name} failed: {created:?}");
    }

    assert_eq!(count_entities(conn).ok(), Some(3));
}
```

The workspace warns on `expect_used`; put `#[expect(clippy::expect_used, reason = "tests fail loudly by design")]` on the test function, the same attribute the desktop crate's test modules use.

- [ ] **Step 2: Run them**

Run: `cargo test -p oikonomia-core prefs_file_from_the_paid_build_still_loads a_vault_holds_any_number_of_entities`
Expected: PASS (run each filter separately if cargo rejects two filters).

- [ ] **Step 3: Delete licensing from core**

- `git rm crates/oikonomia-core/src/license.rs crates/oikonomia-core/tests/license_flow.rs`
- `lib.rs`: delete `pub mod license;`.
- `ledger/entities.rs`: delete `use crate::license::LicenseVerifier;`, `use std::path::Path;` (confirm no other use in the file), and the whole `create_entity_allowed` function with its doc comment. Keep `count_entities`.
- `ledger/mod.rs`: remove `create_entity_allowed` from the re-export list.
- `error.rs`: delete the `LicenseInvalid`, `LicenseExpired`, `LicenseEntityLimit` variants with their doc comments.
- `prefs.rs`: delete the `trial_started_at` field with its doc comment, the `trial_started_at: None,` initializer in the round-trip test, and the `assert_eq!(prefs.trial_started_at, None);` line in `missing_last_used_fields_default`.
- `crates/oikonomia-core/Cargo.toml`: delete both `ed25519-dalek = { workspace = true }` lines.
- Find and delete the `workspace_pins_ed25519_dalek_v3` test: `grep -rn workspace_pins_ed25519 crates`.
- `grep -rniE 'licen[sc]e|trial_start' crates/oikonomia-core/src crates/oikonomia-core/tests` must show only doc text unrelated to product licensing; fix any stale comment it finds (for example in `util.rs`).

- [ ] **Step 4: Delete the mint crate and the workspace dependency**

- `git rm -r crates/oikonomia-mint`
- Root `Cargo.toml`: remove `"crates/oikonomia-mint",` from `members`; delete the four comment lines above `ed25519-dalek = "3.0"` and that line; delete the `getrandom = "0.4"` entry and its two comment lines (the mint crate was its only user).
- `deny.toml`: `grep -n 'mint\|ed25519\|dalek' deny.toml` and remove any entry that names the deleted crate or dependency.
- Run `cargo build --workspace` so `Cargo.lock` drops the unused packages.

- [ ] **Step 5: Remove the error codes from the shell and the web**

- `apps/desktop/src-tauri/src/error.rs`: delete the three `CoreError::License*` match arms and the three sample entries in the test.
- `web/src/lib/errorCodes.json`: delete `"license_invalid"`, `"license_expired"`, `"license_entity_limit"`.
- `web/src/lib/commandError.ts`: delete the three `license_*` map entries.
- Locale files: delete `error.licenseInvalid`, `error.licenseExpired`, `error.licenseEntityLimit` from all four.

- [ ] **Step 6: Run both gates**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
scripts/assert-core-offline.sh
cd web && npx tsc -b && npm run lint && npm test
```
Expected: all pass. Then:

```bash
git grep -niE 'licen[sc]e|paddle|eula|trial' -- crates apps web/src web/marketing \
  | grep -viE 'trial.?balance|reports\.trial|rpt\.|OFL|license-file|licenseFile|\[licenses|deny.toml'
```
Expected: no output. Remaining `license-file`, `licenseFile`, and `EULA.md` itself belong to PR 2.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(core): delete the licensing subsystem and the mint crate

Removes license verification, the trial clock, the one-entity cap, the
three License error variants, and the operator-only minting CLI, along
with ed25519-dalek, which only they used. Old prefs files that still
carry trial_started_at keep loading; a test pins that."
```

- [ ] **Step 8: Open PR 1**

Run the review required by the global rules (adversarial: bugs, missing tests, API surface, dependency risk), then `gh pr create` with a body that states: what was removed, that writes are no longer gated, that leftover license state on disk is ignored, and the verification commands run.

---

# PR 2: open-source the repository

Branch: `chore/open-source` from `main` after PR 1 merges.

### Task 4: GPL licence metadata

**Files:**
- Create: `LICENSE`
- Delete: `EULA.md`
- Modify: `Cargo.toml`, each crate `Cargo.toml` (`crates/oikonomia-core`, `crates/oikonomia-update`, `crates/macos-dock-icon`, `apps/desktop/src-tauri`), `deny.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/src-tauri/src/config_checks.rs`, `web/package.json`

**Interfaces:**
- Produces: `LICENSE` at the repo root; `license = "GPL-3.0-or-later"` on every workspace crate.

- [ ] **Step 1: Write the failing test**

Append to `apps/desktop/src-tauri/src/config_checks.rs`:

```rust
#[test]
fn bundle_ships_the_gpl_licence_text() {
    assert_eq!(config()["bundle"]["licenseFile"], "../../../LICENSE");

    let licence = include_str!("../../../../LICENSE");
    assert!(
        licence
            .trim_start()
            .starts_with("GNU GENERAL PUBLIC LICENSE"),
        "LICENSE is not the GPL text"
    );
    assert!(licence.contains("Version 3, 29 June 2007"));

    assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-or-later");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p oikonomia bundle_ships_the_gpl_licence_text`
Expected: compile error, `LICENSE` not found.

- [ ] **Step 3: Add the licence text and metadata**

```bash
curl -fsSL https://www.gnu.org/licenses/gpl-3.0.txt -o LICENSE
head -2 LICENSE     # "GNU GENERAL PUBLIC LICENSE" / "Version 3, 29 June 2007"
wc -l LICENSE       # 674
git rm EULA.md
```

Do not edit the text.

- Root `Cargo.toml`: replace `license-file = "EULA.md"` with `license = "GPL-3.0-or-later"`.
- In each of the four crate manifests: replace `license-file.workspace = true` with `license.workspace = true`, and replace the three-line proprietary comment above `publish = false` with `# Distributed as an application, not as a crates.io library.`
- `deny.toml`: keep `[licenses.private] ignore = true` (workspace crates are `publish = false`, so cargo-deny skips them and the dependency allow list stays free of GPL). Replace its comment with:

```toml
[licenses.private]
# Workspace crates are GPL-3.0-or-later and `publish = false`. Ignoring them
# here keeps GPL out of the dependency allow list above: every dependency
# must stay permissive or MPL-2.0, all of which are GPL-3.0 compatible.
ignore = true
```

- `tauri.conf.json`: `"licenseFile": "../../../LICENSE"`.
- `web/package.json`: add `"license": "GPL-3.0-or-later",` after `"version"`.

- [ ] **Step 4: Run the gate**

Run:
```bash
cargo test -p oikonomia config_checks
cargo deny check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: relicense under GPL-3.0-or-later

Oikonomia is now free software. The EULA is replaced by the GPL text,
which the bundle ships; the dependency allow list is unchanged so no
copyleft dependency can slip in unnoticed."
```

### Task 5: Public documentation

**Files:**
- Create: `CONTRIBUTING.md`, `SECURITY.md`
- Delete: `docs/commerce.md`
- Modify: `README.md`, `AGENTS.md`, `docs/release.md`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `docs/superpowers/specs/2026-09-01-go-to-market-design.md`, `docs/superpowers/plans/2026-09-01-go-to-market.md`

- [ ] **Step 1: Remove the commerce docs**

- `git rm docs/commerce.md`
- `docs/release.md`:
  - In the Environment step 4, end the sentence at `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).` and drop "That is the updater minisign key, not the license key."
  - "What never lives on GitHub": delete the three bullets about the license-signing key, Paddle, and buyer PII, leaving the Authenticode bullet. In the paragraph after it delete "It is not the license signing key." and "That key is not the license Ed25519 key."
  - "Key ceremony": replace the intro with `The updater keypair is generated offline; its private half never touches the repository.` Delete the sections "License Ed25519 keypair" and "Before first sale (go-live checklist item 4)".
  - In "Cut a build" step 4, change `HTTP is not on \`oikonomia-core\` or \`license.rs\`` to `HTTP is not on \`oikonomia-core\``.
- `.github/workflows/ci.yml` line 23 and `.github/workflows/release.yml` lines 11, 52, 63-65, 70, 143, 254, 331: delete every clause or line that mentions the license signing key, Paddle, or buyer PII. A sentence that becomes empty is deleted; the `echo`/`Write-Host` lines become `This is the updater minisign key.`
- Verify: `git grep -niE 'paddle|license (signing )?key|buyer|mint' -- docs/release.md .github` prints nothing.

- [ ] **Step 2: Update `README.md`**

- Replace the paragraph beginning "Local-only personal and company finance" with:

```markdown
Free and open-source, local-only personal and company finance: **double-entry**
multi-entity books, **encrypted at rest**, with a dark "Aurora glass" desktop UI.
No account, no cloud, no telemetry.
```

- Rename the heading `## v1 features` to `## Features`. Delete the two bullets "Offline licensing: ..." and "Global trial banner ...".
- Support section: change "Questions, bug reports, license problems, and security reports" to "Questions and bug reports". Add: `Security reports: see [SECURITY.md](SECURITY.md).`
- Security notes: change "The vault, ledger, and license paths" to "The vault and ledger paths".
- Layout table: delete the `crates/oikonomia-mint` row.
- Backlog line: delete "webhook fulfillment,".
- Add before `## Layout`:

```markdown
## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Business rules live in Rust
(`oikonomia-core`); the React UI only renders.

## License

Oikonomia is free software, released under the
[GNU General Public License v3.0 or later](LICENSE).
Copyright (c) 2026 Ourovoros.io.

Bundled fonts are under the SIL Open Font License; see `web/public/fonts`.
```

- [ ] **Step 3: Write `CONTRIBUTING.md`**

```markdown
# Contributing to Oikonomia

Thanks for helping. Oikonomia is a local-only, encrypted, double-entry finance
app. Changes are judged first on whether they keep those three properties.

## Setup

- Rust stable 1.94 or newer, Node 22 or newer
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```bash
cargo tauri dev          # run the desktop app
make check               # the full gate CI runs
```

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
cd web && npx tsc -b && npm run lint && npm test
```

Behaviour changes need tests. Keep commits small and explain why in the
message.

## Rules that are not negotiable

- Money is integer minor units (`i64`). Never a float.
- Every posted journal entry balances: debits equal credits, at least two lines.
- Business rules live in `oikonomia-core`, never in TypeScript.
- `oikonomia-core` has no network code. The only network path is the
  click-driven update check in `oikonomia-update`.
- The vault is encrypted at rest. No plaintext database on disk.
- No new dependency without a reason a small local implementation cannot meet.
  New dependencies must be permissively licensed; `cargo deny check` enforces it.

`AGENTS.md` lists the full set of invariants.

## Licence

By contributing you agree that your contribution is licensed under
GPL-3.0-or-later, the same licence as the project.

## Security issues

Do not open a public issue. See [SECURITY.md](SECURITY.md).
```

- [ ] **Step 4: Write `SECURITY.md`**

```markdown
# Security policy

## Reporting a vulnerability

Email **info@ourovoros.io**. Please do not open a public issue for a
vulnerability. Never attach a vault or backup file to a report.

Include the app version (Settings, or the unlock screen), your operating
system, and steps to reproduce. You will get an acknowledgement, and a fix or
a decision is published in the release notes.

## Supported versions

Only the latest release receives security fixes.

## What Oikonomia protects

- The vault is a SQLCipher database keyed from your master password with
  Argon2id. A stolen disk, or a stolen backup file, does not reveal your books.
- The app makes no background network connection. The only network action is
  the update check you click on the unlock screen, which verifies a minisign
  signature before anything is installed.

## What it does not protect

- Malware running as you while the vault is unlocked, keyloggers, and memory
  forensics on a running app.
- A lost master password. There is no recovery key.
```

- [ ] **Step 5: Update `AGENTS.md` and mark superseded docs**

- `AGENTS.md`: replace the opening description's "(Stripe-inspired, dark mode)" with "(dark Aurora glass)"; add layout rows for `crates/oikonomia-update` ("Signed update check, the only network path") and `crates/macos-dock-icon` ("macOS Dock icon for `cargo tauri dev`").
- At the very top of `docs/superpowers/specs/2026-09-01-go-to-market-design.md` and `docs/superpowers/plans/2026-09-01-go-to-market.md`, insert:

```markdown
> **Superseded (2026-10-01).** Oikonomia is no longer sold. The commerce,
> licensing, and EULA parts of this document were removed by
> `docs/superpowers/specs/2026-10-01-open-source-donations-design.md`.
> The release lane, updater, and code signing described here still apply.
```

- [ ] **Step 6: Verify and commit**

Run: `git grep -niE 'paddle|eula|buy a license|\.lic\b|oikonomia-mint' -- . ':!docs/superpowers'`
Expected: no output.

```bash
git add -A
git commit -m "docs: rewrite public docs for the open-source release

Drops the Paddle fulfilment runbook and the license-key ceremony, and
adds CONTRIBUTING and SECURITY so outside contributors know the rules
and where to report vulnerabilities."
```

### Task 6: Pre-publication audit

No code change. The output is a report to the owner; nothing is made public by this task.

- [ ] **Step 1: Scan the full history for secrets**

```bash
brew install gitleaks
gitleaks git --redact --report-format json --report-path "$TMPDIR/oikonomia-gitleaks.json" .
gitleaks dir --redact .
```

Expected: `no leaks found` for both. Triage every finding: a public key (minisign updater pubkey, the old license pubkey) is not a secret; anything that is a private key, token, or password is a blocker.

- [ ] **Step 2: Check for content that should not be public**

```bash
git log --all --format='%ae' | sort -u
git grep -nIE '/Users/|N7GGW2F27L|@proton\.me|RELEASES_REPO_TOKEN=' -- . ':!docs/superpowers'
git ls-files | grep -iE '\.(p12|pem|key|lic|env)$'
```

Expected: the author list contains only addresses the owner is content to publish; the Apple team ID appearing in signing config is expected and public; no key or env files are tracked.

- [ ] **Step 3: Report**

Give the owner: the gitleaks result, the author email list, and any finding from Step 2. State plainly that a real secret found in history is not fixed by a new commit: the secret must be rotated, and the owner then chooses between rewriting history and publishing a fresh single-commit repository. Do not change the repository's visibility.

- [ ] **Step 4: Open PR 2**

Run the adversarial review, then `gh pr create`. The body lists the licence change, the deleted commerce docs, and the audit result.

---

# PR 3: donations

Branch: `feat/donations` from `main` after PR 2 merges.

**Blocked input:** the owner supplies six receiving addresses (BTC, ETH, XMR, DASH, LTC, SOL). Do not start Task 7 without them, and never invent or copy an address from anywhere else. In the code below, each `OWNER_*` token stands for the literal address string the owner gave.

### Task 7: Donation address table and command

**Files:**
- Create: `apps/desktop/src-tauri/src/donations.rs`, `.github/FUNDING.yml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`, `README.md`

**Interfaces:**
- Produces: IPC command `donation_addresses` returning `Vec<DonationAddress>`, serialized as
  `[{ "coin": "BTC", "network": "Bitcoin", "also_accepts": [], "address": "..." }, ...]`.
  `coin` is one of `"BTC" | "ETH" | "XMR" | "DASH" | "LTC" | "SOL"`. The command needs no unlocked vault.

- [ ] **Step 1: Write the module with its tests and an empty table**

Create `apps/desktop/src-tauri/src/donations.rs`:

```rust
//! Donation addresses. Oikonomia is free; these are the only way to pay for it.
//!
//! The table is the single source of truth: the Settings page renders what
//! [`donation_addresses`] returns, and a test keeps `README.md` in step.
//! Nothing here touches the network.

use serde::Serialize;

/// A coin with its own receiving address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum Coin {
    /// Bitcoin.
    Btc,
    /// Ether, on Ethereum mainnet.
    Eth,
    /// Monero.
    Xmr,
    /// Dash.
    Dash,
    /// Litecoin.
    Ltc,
    /// SOL, on Solana.
    Sol,
}

/// One receiving address, as shown in Settings and the README.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct DonationAddress {
    /// The coin this address belongs to.
    pub(crate) coin: Coin,
    /// Network name, shown so a donor does not send on the wrong chain.
    pub(crate) network: &'static str,
    /// Tokens the same address also receives on this network.
    pub(crate) also_accepts: &'static [&'static str],
    /// The address itself.
    pub(crate) address: &'static str,
}

/// Every address donations are accepted on.
pub(crate) const DONATION_ADDRESSES: &[DonationAddress] = &[];

/// Donation addresses for the Settings page. Readable while locked.
#[tauri::command]
pub fn donation_addresses() -> Vec<DonationAddress> {
    DONATION_ADDRESSES.to_vec()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use std::collections::HashSet;

    use super::{Coin, DONATION_ADDRESSES};

    const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    const BECH32: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";

    fn all_in(text: &str, alphabet: &str) -> bool {
        !text.is_empty() && text.chars().all(|character| alphabet.contains(character))
    }

    fn is_base58_with(address: &str, prefixes: &[char], lengths: std::ops::RangeInclusive<usize>) -> bool {
        all_in(address, BASE58)
            && lengths.contains(&address.len())
            && address.starts_with(prefixes)
    }

    fn is_bech32_with(address: &str, prefix: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
        address
            .strip_prefix(prefix)
            .is_some_and(|data| all_in(data, BECH32))
            && lengths.contains(&address.len())
    }

    /// Whether `address` has the right shape for `coin`. A format check, not
    /// a checksum: it catches a typo, a truncation, or the wrong coin's
    /// address in a row. It cannot tell whose address it is.
    fn has_valid_shape(coin: Coin, address: &str) -> bool {
        match coin {
            Coin::Btc => {
                is_bech32_with(address, "bc1", 42..=62)
                    || is_base58_with(address, &['1', '3'], 26..=35)
            }
            Coin::Ltc => {
                is_bech32_with(address, "ltc1", 43..=63)
                    || is_base58_with(address, &['L', 'M', '3'], 26..=35)
            }
            Coin::Dash => is_base58_with(address, &['X', '7'], 34..=34),
            Coin::Xmr => {
                is_base58_with(address, &['4', '8'], 95..=95)
                    || is_base58_with(address, &['4'], 106..=106)
            }
            Coin::Sol => all_in(address, BASE58) && (32..=44).contains(&address.len()),
            Coin::Eth => address.strip_prefix("0x").is_some_and(|hex| {
                hex.len() == 40 && hex.chars().all(|character| character.is_ascii_hexdigit())
            }),
        }
    }

    #[test]
    fn shape_check_accepts_well_formed_addresses() {
        assert!(has_valid_shape(Coin::Btc, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"));
        assert!(has_valid_shape(Coin::Btc, "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"));
        assert!(has_valid_shape(Coin::Eth, "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe"));
        assert!(has_valid_shape(Coin::Sol, "11111111111111111111111111111111"));
        assert!(has_valid_shape(Coin::Dash, &format!("X{}", "a".repeat(33))));
        assert!(has_valid_shape(Coin::Ltc, &format!("L{}", "a".repeat(33))));
        assert!(has_valid_shape(Coin::Xmr, &format!("4{}", "A".repeat(94))));
        assert!(has_valid_shape(Coin::Xmr, &format!("8{}", "A".repeat(94))));
    }

    #[test]
    fn shape_check_rejects_damaged_or_misplaced_addresses() {
        let bitcoin = "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq";
        let ether = "0xde0B295669a9FD93d5F28D9Ec85E40f4cb697BAe";

        // Whitespace from a careless paste.
        assert!(!has_valid_shape(Coin::Btc, &format!("{bitcoin}\n")));
        assert!(!has_valid_shape(Coin::Eth, &format!(" {ether}")));

        // Truncated, empty, or a character outside the alphabet.
        assert!(!has_valid_shape(Coin::Eth, &ether[..41]));
        assert!(!has_valid_shape(Coin::Btc, ""));
        assert!(!has_valid_shape(Coin::Sol, "0OIl0OIl0OIl0OIl0OIl0OIl0OIl0OIl"));
        assert!(!has_valid_shape(Coin::Xmr, &format!("4{}", "A".repeat(93))));

        // The right address in the wrong row.
        assert!(!has_valid_shape(Coin::Ltc, bitcoin));
        assert!(!has_valid_shape(Coin::Btc, ether));
        assert!(!has_valid_shape(Coin::Dash, "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"));
    }

    #[test]
    fn table_lists_each_coin_once_with_a_well_formed_address() {
        let coins: Vec<Coin> = DONATION_ADDRESSES.iter().map(|entry| entry.coin).collect();
        assert_eq!(
            coins,
            [Coin::Btc, Coin::Eth, Coin::Xmr, Coin::Dash, Coin::Ltc, Coin::Sol]
        );

        for entry in DONATION_ADDRESSES {
            assert!(
                has_valid_shape(entry.coin, entry.address),
                "{:?} address is malformed: {:?}",
                entry.coin,
                entry.address
            );
            assert!(!entry.network.is_empty());
        }

        let unique: HashSet<&str> = DONATION_ADDRESSES.iter().map(|entry| entry.address).collect();
        assert_eq!(unique.len(), DONATION_ADDRESSES.len(), "duplicate address");
    }

    #[test]
    fn stablecoins_ride_on_the_ether_and_solana_addresses_only() {
        for entry in DONATION_ADDRESSES {
            let expected: &[&str] = match entry.coin {
                Coin::Eth | Coin::Sol => &["USDC", "USDT"],
                _ => &[],
            };
            assert_eq!(entry.also_accepts, expected, "{:?}", entry.coin);
        }
    }

    #[test]
    fn readme_lists_every_address() {
        let readme = include_str!("../../../../README.md");

        for entry in DONATION_ADDRESSES {
            assert!(
                readme.contains(entry.address),
                "README.md is missing the {:?} address",
                entry.coin
            );
        }
    }

    #[test]
    fn payload_uses_ticker_symbols() {
        let json = serde_json::to_value(super::donation_addresses()).expect("serialize");
        assert_eq!(json[0]["coin"], "BTC");
        assert_eq!(json[1]["also_accepts"][0], "USDC");
    }
}
```

In `lib.rs` add `mod donations;` (alphabetical, after `config_checks`) and register `donations::donation_addresses,` after `commands::open_support_email,`.

- [ ] **Step 2: Run the tests to verify the table tests fail**

Run: `cargo test -p oikonomia donations`
Expected: the two `shape_check_*` tests PASS. `table_lists_each_coin_once_with_a_well_formed_address` and `payload_uses_ticker_symbols` FAIL (empty table).

- [ ] **Step 3: Fill the table with the owner's addresses**

Replace the empty constant:

```rust
/// Every address donations are accepted on.
pub(crate) const DONATION_ADDRESSES: &[DonationAddress] = &[
    DonationAddress {
        coin: Coin::Btc,
        network: "Bitcoin",
        also_accepts: &[],
        address: "OWNER_BTC_ADDRESS",
    },
    DonationAddress {
        coin: Coin::Eth,
        network: "Ethereum",
        also_accepts: &["USDC", "USDT"],
        address: "OWNER_ETH_ADDRESS",
    },
    DonationAddress {
        coin: Coin::Xmr,
        network: "Monero",
        also_accepts: &[],
        address: "OWNER_XMR_ADDRESS",
    },
    DonationAddress {
        coin: Coin::Dash,
        network: "Dash",
        also_accepts: &[],
        address: "OWNER_DASH_ADDRESS",
    },
    DonationAddress {
        coin: Coin::Ltc,
        network: "Litecoin",
        also_accepts: &[],
        address: "OWNER_LTC_ADDRESS",
    },
    DonationAddress {
        coin: Coin::Sol,
        network: "Solana",
        also_accepts: &["USDC", "USDT"],
        address: "OWNER_SOL_ADDRESS",
    },
];
```

- [ ] **Step 4: Add the README section and FUNDING.yml**

Insert in `README.md` before `## Contributing`, with the same six literal addresses:

```markdown
## Donate

Oikonomia is free. If it is useful to you, a donation helps keep it maintained.
Check the coin and network before sending: crypto transfers cannot be reversed.

| Coin | Network | Address |
|------|---------|---------|
| BTC | Bitcoin | `OWNER_BTC_ADDRESS` |
| ETH, USDC, USDT | Ethereum | `OWNER_ETH_ADDRESS` |
| XMR | Monero | `OWNER_XMR_ADDRESS` |
| DASH | Dash | `OWNER_DASH_ADDRESS` |
| LTC | Litecoin | `OWNER_LTC_ADDRESS` |
| SOL, USDC, USDT | Solana | `OWNER_SOL_ADDRESS` |

The same addresses are shown in the app under Settings, Donate.
```

Create `.github/FUNDING.yml`:

```yaml
custom: ["https://github.com/ourovoros-io/oikonomia#donate"]
```

- [ ] **Step 5: Run the gate**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p oikonomia
```
Expected: all pass, including all six `donations` tests.

- [ ] **Step 6: Owner confirms the addresses**

Print the table (`grep -n 'address:' apps/desktop/src-tauri/src/donations.rs`) and ask the owner to compare each line, character by character, against their wallet. The tests prove format only. Do not commit until the owner confirms.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(desktop): donation address table and command

Donations are the only way to pay for Oikonomia now. One Rust table
feeds the app and is checked against the README, and format tests stop
a mistyped or misplaced address from shipping."
```

### Task 8: Donate section in Settings

**Files:**
- Create: `web/src/components/DonationAddresses.tsx`, `web/src/components/DonationAddresses.test.tsx`
- Modify: `web/src/lib/api.ts`, `web/src/pages/SettingsPage.tsx`, `web/src/pages/SettingsPage.test.tsx`, `web/src/pages/SettingsPage.i18n.test.tsx`, `web/src/App.test.tsx`, `web/src/locales/{en,el,fr,de}.json`, `web/marketing/fixture/handler.ts`

**Interfaces:**
- Consumes: IPC `donation_addresses` from Task 7.
- Produces: `api.donationAddresses(): Promise<DonationAddress[]>` and

```ts
export type DonationAddress = {
  coin: 'BTC' | 'ETH' | 'XMR' | 'DASH' | 'LTC' | 'SOL'
  network: string
  also_accepts: string[]
  address: string
}
```

- [ ] **Step 1: Add the strings**

Add to `en.json` (flat keys) and the same keys nested under `settings.donate` in the other three:

| Key | en | el | fr | de |
|-----|----|----|----|----|
| `settings.donate.title` | Donate | Δωρεά | Faire un don | Spenden |
| `settings.donate.description` | Oikonomia is free and open source. Donations are optional. | Το Oikonomia είναι δωρεάν και ανοιχτού κώδικα. Οι δωρεές είναι προαιρετικές. | Oikonomia est gratuit et open source. Les dons sont facultatifs. | Oikonomia ist kostenlos und quelloffen. Spenden sind freiwillig. |
| `settings.donate.warning` | Check the coin and network before sending. Crypto transfers cannot be reversed. | Ελέγξτε το νόμισμα και το δίκτυο πριν από την αποστολή. Οι μεταφορές κρυπτονομισμάτων δεν αναιρούνται. | Vérifiez la monnaie et le réseau avant l'envoi. Les transferts de cryptomonnaies sont irréversibles. | Prüfen Sie Währung und Netzwerk vor dem Senden. Krypto-Überweisungen lassen sich nicht rückgängig machen. |
| `settings.donate.alsoAccepts` | Also accepts {tokens} | Δέχεται επίσης {tokens} | Accepte aussi {tokens} | Akzeptiert auch {tokens} |
| `settings.donate.copy` | Copy {coin} address | Αντιγραφή διεύθυνσης {coin} | Copier l'adresse {coin} | {coin}-Adresse kopieren |
| `settings.donate.copied` | Copied | Αντιγράφηκε | Copié | Kopiert |
| `settings.donate.copyFailed` | Could not copy. Select the address and copy it by hand. | Η αντιγραφή απέτυχε. Επιλέξτε τη διεύθυνση και αντιγράψτε την χειροκίνητα. | Copie impossible. Sélectionnez l'adresse et copiez-la manuellement. | Kopieren fehlgeschlagen. Markieren Sie die Adresse und kopieren Sie sie von Hand. |

- [ ] **Step 2: Write the failing component test**

Create `web/src/components/DonationAddresses.test.tsx`:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'
import type { DonationAddress } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DonationAddresses } from './DonationAddresses'

const addresses: DonationAddress[] = [
  { coin: 'BTC', network: 'Bitcoin', also_accepts: [], address: 'bc1qexampleexampleexample' },
  {
    coin: 'ETH',
    network: 'Ethereum',
    also_accepts: ['USDC', 'USDT'],
    address: '0x00000000000000000000000000000000000000aa',
  },
]

function stubClipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
  })
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('DonationAddresses', () => {
  test('shows one row per address with coin, network, and stablecoin note', () => {
    render(<DonationAddresses addresses={addresses} />)

    expect(screen.getByText('bc1qexampleexampleexample')).toBeTruthy()
    expect(screen.getByText('0x00000000000000000000000000000000000000aa')).toBeTruthy()
    expect(screen.getByText('Bitcoin')).toBeTruthy()
    expect(screen.getByText('Also accepts USDC, USDT')).toBeTruthy()
    expect(screen.getAllByText(/also accepts/i)).toHaveLength(1)
    expect(screen.getByText(/cannot be reversed/i)).toBeTruthy()
  })

  test('copy writes exactly the address and confirms', async () => {
    const writeText = vi.fn(async () => {})
    stubClipboard(writeText)
    render(<DonationAddresses addresses={addresses} />)

    await userEvent.click(screen.getByRole('button', { name: 'Copy ETH address' }))

    expect(writeText).toHaveBeenCalledWith('0x00000000000000000000000000000000000000aa')
    expect(await screen.findByText('Copied')).toBeTruthy()
  })

  test('a refused clipboard write tells the user to copy by hand', async () => {
    stubClipboard(vi.fn(async () => Promise.reject(new Error('denied'))))
    render(<DonationAddresses addresses={addresses} />)

    await userEvent.click(screen.getByRole('button', { name: 'Copy BTC address' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(/copy it by hand/i)
    expect(screen.queryByText('Copied')).toBeNull()
  })

  test('renders nothing when there are no addresses', () => {
    const { container } = render(<DonationAddresses addresses={[]} />)
    expect(container).toBeEmptyDOMElement()
  })
})
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cd web && npx vitest run src/components/DonationAddresses.test.tsx`
Expected: FAIL, module `./DonationAddresses` not found.

- [ ] **Step 4: Implement the API call and the component**

`web/src/lib/api.ts`, add the type near the other exported types and the call next to `openSupportEmail`:

```ts
export type DonationAddress = {
  coin: 'BTC' | 'ETH' | 'XMR' | 'DASH' | 'LTC' | 'SOL'
  network: string
  also_accepts: string[]
  address: string
}
```

```ts
  /** Donation addresses from the Rust table. Empty outside Tauri. */
  donationAddresses: () =>
    isTauri() ? call<DonationAddress[]>('donation_addresses') : Promise.resolve([]),
```

Create `web/src/components/DonationAddresses.tsx`:

```tsx
import { Check, Copy } from 'lucide-react'
import { useState } from 'react'
import type { DonationAddress } from '../lib/api'
import { t } from '../lib/i18n'
import { Button, ErrorBanner } from './ui'

/** Donation addresses from Rust. Renders and copies; holds no rules. */
export function DonationAddresses({ addresses }: { addresses: DonationAddress[] }) {
  const [copied, setCopied] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  if (addresses.length === 0) return null

  async function onCopy(entry: DonationAddress) {
    try {
      await navigator.clipboard.writeText(entry.address)
      setFailed(false)
      setCopied(entry.coin)
    } catch {
      setCopied(null)
      setFailed(true)
    }
  }

  return (
    <div className="space-y-3">
      <p className="text-xs leading-snug text-[var(--color-muted)]">
        {t('settings.donate.warning')}
      </p>

      <ul className="divide-y divide-[var(--color-border)]">
        {addresses.map((entry) => (
          <li key={entry.coin} className="flex items-center gap-3 py-3">
            <div className="w-24 shrink-0">
              <div className="text-sm font-medium text-[var(--color-fg)]">{entry.coin}</div>
              <div className="text-xs text-[var(--color-muted)]">{entry.network}</div>
            </div>

            <div className="min-w-0 flex-1">
              <code className="block select-all break-all font-mono text-xs text-[var(--color-fg-secondary)]">
                {entry.address}
              </code>
              {entry.also_accepts.length > 0 ? (
                <div className="mt-1 text-xs text-[var(--color-muted)]">
                  {t('settings.donate.alsoAccepts', { tokens: entry.also_accepts.join(', ') })}
                </div>
              ) : null}
            </div>

            <Button
              variant="secondary"
              size="iconSm"
              onClick={() => void onCopy(entry)}
              aria-label={t('settings.donate.copy', { coin: entry.coin })}
              title={t('settings.donate.copy', { coin: entry.coin })}
            >
              {copied === entry.coin ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            </Button>
            {copied === entry.coin ? (
              <span role="status" className="text-xs text-[var(--color-muted)]">
                {t('settings.donate.copied')}
              </span>
            ) : null}
          </li>
        ))}
      </ul>

      <ErrorBanner message={failed ? t('settings.donate.copyFailed') : null} />
    </div>
  )
}
```

- [ ] **Step 5: Run the component test**

Run: `cd web && npx vitest run src/components/DonationAddresses.test.tsx`
Expected: 4 PASS.

- [ ] **Step 6: Write the failing Settings test**

In `web/src/pages/SettingsPage.test.tsx` add `donationAddresses: vi.fn(),` to the `api` mock factory, add this to `beforeEach`:

```tsx
  vi.mocked(api.donationAddresses).mockReset().mockResolvedValue([
    { coin: 'BTC', network: 'Bitcoin', also_accepts: [], address: 'bc1qexampleexampleexample' },
  ])
```

and add the test:

```tsx
  test('Donate section sits after Language and lists the Rust addresses', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const titles = screen.getAllByRole('heading', { level: 3 }).map((el) => el.textContent)
    expect(titles.slice(0, 2)).toEqual(['Language', 'Donate'])

    await userEvent.click(screen.getByRole('button', { name: /donate/i }))
    expect(await screen.findByText('bc1qexampleexampleexample')).toBeTruthy()
  })
```

If the section is expanded by default (as the old License section was), drop the `userEvent.click` line. Add the same `donationAddresses: vi.fn(async () => []),` mock line to `SettingsPage.i18n.test.tsx` and `App.test.tsx`.

Run: `cd web && npx vitest run src/pages/SettingsPage.test.tsx -t "Donate section"`
Expected: FAIL, no "Donate" heading.

- [ ] **Step 7: Add the section to `SettingsPage.tsx`**

- Imports: `import { DonationAddresses } from '../components/DonationAddresses'`, add `type DonationAddress` to the `'../lib/api'` import, add `HeartHandshake` to the `lucide-react` import.
- State: `const [donations, setDonations] = useState<DonationAddress[]>([])`
- In the mount `useEffect`, after the `getLockTimeout` chain:

```tsx
    void api
      .donationAddresses()
      .then(setDonations)
      .catch(() => {
        /* ignore: the section stays hidden */
      })
```

- Directly after the Language `CollapsibleSection`, where License used to be:

```tsx
      {donations.length > 0 ? (
        <CollapsibleSection
          title={t('settings.donate.title')}
          description={t('settings.donate.description')}
          icon={<HeartHandshake className="size-4" />}
          tone="muted"
        >
          <DonationAddresses addresses={donations} />
        </CollapsibleSection>
      ) : null}
```

- `web/marketing/fixture/handler.ts`: add `donation_addresses: () => [],` to `answers` next to `app_info`.

- [ ] **Step 8: Run the web gate**

Run: `cd web && npx tsc -b && npm run lint && npm test && npm run build`
Expected: all pass, including locale key parity.

- [ ] **Step 9: Verify in the running app**

Run: `cargo tauri dev`. Open Settings, Donate. Confirm six rows; ETH and SOL show "Also accepts USDC, USDT". Click copy on each row and paste into a text editor: the pasted text must equal the row's address exactly. Switch the language to Greek, French, and German and confirm the section's copy changes. If the copy button shows the failure message in the real webview, stop and report: the fix is a Rust-side clipboard command, which is a design change, not a tweak.

- [ ] **Step 10: Commit and open PR 3**

```bash
git add -A
git commit -m "feat(web): Donate section in Settings

Shows the donation addresses Rust serves, with a copy button per row
and a hand-copy fallback when the webview refuses the clipboard."
```

Run the adversarial review, then `gh pr create`.

---

## After the three PRs

Owner actions, not agent tasks: act on the Task 6 audit report, flip the repository to public, add the description and topics on GitHub, and cut the next release so users on the paid build receive the ungated one through the updater.
