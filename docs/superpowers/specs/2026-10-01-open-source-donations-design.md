# Open source and donations: design

Date: 2026-10-01

## Goal

Oikonomia stops being a paid product. The app becomes free and open source
under GPL-3.0-or-later. The paid licensing subsystem (trial, signed license
files, purchase link, EULA) is removed completely. The only way to give money
is an optional crypto donation to a published receiving address.

This supersedes the commerce parts of
`2026-09-01-go-to-market-design.md`. The release lane, updater, and code
signing from that program stay.

## Decisions

| Topic | Decision |
|-------|----------|
| Licence | GPL-3.0-or-later |
| Licensing code | Deleted outright; no always-allowed stub, no Cargo feature |
| Donations | Shown in the app (Settings) and in the README |
| Donation mechanism | Static receiving addresses only; no processor, no network call |
| Coins | BTC, ETH, XMR, DASH, LTC, SOL; USDC and USDT on Ethereum and Solana |
| Git history | Published as is, provided the secret scan is clean |
| Website | Out of scope; reworked separately |

USDC and USDT are tokens on Ethereum and Solana, so they use the ETH and SOL
addresses. There are six addresses in total.

The history decision was a recommendation the owner did not explicitly
confirm. If the secret scan finds anything, or the owner prefers a fresh
single-commit public repo, that changes only the final publishing step, not
the three PRs below.

## Non-goals

- QR codes, fiat price display, donation tracking, or any thank-you state.
- Any new network access. The app stays offline apart from the click-driven
  updater.
- Cleaning up leftover license state on existing machines.
- The marketing website.
- Making the repository public. That is the owner's action after PR 2.

## Delivery

Three pull requests, in order. Each leaves `main` green and shippable.

### PR 1: remove licensing

No behaviour is gated after this PR. Every write that an expired trial used to
refuse succeeds, and a vault may hold any number of entities.

Core (`crates/oikonomia-core`):

- Delete `src/license.rs` and `tests/license_flow.rs`; remove `pub mod license`.
- `ledger/entities.rs`: the gated entity-creation entry point loses its
  verifier and data-dir parameters and its call to
  `require_entity_create_allowed`. If that leaves it identical to
  `create_entity`, delete the wrapper and move callers to `create_entity`.
- `error.rs`: remove `LicenseInvalid`, `LicenseExpired`, `LicenseEntityLimit`.
- `prefs.rs`: remove `trial_started_at`. `UiPrefs` is `#[serde(default)]`
  without `deny_unknown_fields`, so an old prefs file carrying the key still
  loads. A test pins that.
- Remove `ed25519-dalek` from the crate and the workspace, along with the
  `workspace_pins_ed25519_dalek_v3` test. Nothing else uses it.

Mint:

- Delete `crates/oikonomia-mint` and its workspace member entry.

Desktop (`apps/desktop/src-tauri`):

- Delete `trial_store.rs`, the `license_status`, `license_install`, and
  `eula_text` commands, `LicenseStatusPayload`, `BUY_URL`, and
  `stamp_trial_start` with its calls in vault init and unlock.
- Delete the license-gated vault wrapper; its callers use
  `with_vault_blocking`.
- `tray.rs`: delete `license_filter_label` and its test.
- `error.rs`: remove the three license error codes.
- `update.rs`: the test that the updater key differs from the license key goes
  with the license key; the non-empty minisign key assertion stays.
- Any keychain dependency used only by `trial_store.rs` is removed.

Web (`web`):

- Delete `components/TrialBanner.tsx`, `lib/license.ts`, and their tests.
- `App.tsx`: remove the license state, the status fetch, and the banner.
- `SettingsPage.tsx`: remove the License section and the EULA viewer.
- `lib/api.ts`, `lib/commandError.ts`, `lib/errorCodes.json`: remove the
  license calls and codes.
- `locales/{en,el,fr,de}.json`: remove `settings.license.*`,
  `settings.trial.*`, top-level `trial.*`, and the license error strings.
  `rpt.trialBalance` and other accounting uses of "trial" stay.
- `marketing/fixture`: remove the mocked license commands.

Existing installs: a leftover `license.lic`, the macOS Keychain trial stamp,
and the old pref key are ignored. Nothing reads or deletes them.

Tests:

- Core: creating a second and third entity succeeds; a write succeeds in a
  data directory with no license file and no prefs.
- Core: a prefs file containing `trial_started_at` deserializes.
- Desktop: the registered command list contains no `license_*` or `eula_text`.
- Web: Settings renders with no License section; i18n key parity across the
  four locales still holds.

### PR 2: open-source the repository

Licence metadata:

- Add `LICENSE` with the unmodified GPL-3.0 text. Delete `EULA.md`.
- Workspace `Cargo.toml`: replace `license-file = "EULA.md"` with
  `license = "GPL-3.0-or-later"`; each crate uses `license.workspace = true`
  and drops the proprietary-licence comment. `publish = false` stays.
- `web/package.json`: add `"license": "GPL-3.0-or-later"`.
- `tauri.conf.json`: `licenseFile` points at `LICENSE`.
- `deny.toml`: remove `[licenses.private]`, add `GPL-3.0-or-later` to the
  allow list for the workspace crates, and confirm `cargo deny check licenses`
  passes. Every licence already on the allow list is GPL-3.0 compatible;
  the check is re-verified against the actual graph, including vendored
  OpenSSL (Apache-2.0) under SQLCipher.

Documentation:

- Delete `docs/commerce.md`.
- `docs/release.md`: remove the license Ed25519 ceremony and every Paddle and
  buyer-PII reference. The updater minisign ceremony stays.
- `.github/workflows/*.yml`: remove comments about the license signing key
  and Paddle.
- `README.md`: rewrite for a public audience: what the app is, features,
  build from source, security model, contributing, licence. Remove the
  licensing, trial, and buy-link lines and the mint crate row.
- `AGENTS.md`: update the layout table.
- Add `CONTRIBUTING.md` (build, test, lint commands, the invariants from
  `AGENTS.md`, the "business logic lives in Rust" rule) and `SECURITY.md`
  (report privately to info@ourovoros.io; scope and the offline guarantee).
- Mark `2026-09-01-go-to-market-design.md` and its plan as superseded at the
  top, pointing here. They stay in the tree as history.

Capability scope:

- The opener permission is scoped to `https://ourovoros.io/oikonomia*`, the
  former buy page. Re-scope it to the repository URL
  `https://github.com/ourovoros-io/oikonomia*`. `open_support_email` keeps
  building the mailto in Rust. The existing test that forbids mailto globs
  and extra URL globs is updated to the new single glob.

Pre-publication audit:

- Run a secret scanner (gitleaks) over the full history and the working tree.
  Report findings to the owner before anything is made public. A finding
  blocks publication and is not fixed by a later commit alone.
- Check the tree for personal or machine-specific content that should not be
  public.

### PR 3: donations

Blocked on the owner supplying six addresses.

Data (Rust, desktop crate, new `donations.rs`):

```rust
pub(crate) struct DonationAddress {
    pub(crate) coin: Coin,
    pub(crate) network: &'static str,
    pub(crate) also_accepts: &'static [&'static str],
    pub(crate) address: &'static str,
}
```

`Coin` is an enum of the six base coins. A single `const DONATION_ADDRESSES`
table holds the rows. The ETH and SOL rows carry
`also_accepts: &["USDC", "USDT"]`. A `donation_addresses` command returns the
table serialized; it needs no unlocked vault.

UI:

- Settings gets a "Support Oikonomia" section in the position the License
  section held. One row per address: coin, network, the "also accepts" note,
  the address in the mono font, and a copy button.
- The copy button writes to the clipboard and shows a short confirmation.
  No URL is opened and no network request is made.
- Strings are localized in the four locales. Addresses and ticker symbols are
  not translated.

README:

- A "Donate" section lists the same six rows. A desktop-crate test reads
  `README.md` and asserts each table address appears in it, so the two cannot
  drift.
- `.github/FUNDING.yml` uses `custom` to link to the README section.

Validation (tests, no new dependencies):

- Each address is non-empty, has no surrounding whitespace, and matches the
  expected shape for its coin: prefix, length range, and character set
  (bech32 or base58 for BTC and LTC, `0x` plus 40 hex for ETH, base58 with
  the `4` or `8` prefix and 95 or 106 characters for XMR, `X` prefix base58
  for DASH, base58 of 32 to 44 characters for SOL).
- No address appears twice.

These are shape checks, not checksum verification. The owner confirms each
address against their wallet before merge.

## Risks

- Removing the gate changes behaviour for anyone on an expired trial: writes
  start working. That is the intent.
- GPL applies to the bundled binary. Dependencies are all permissive or
  MPL-2.0; `cargo deny` is the gate.
- A wrong donation address is unrecoverable for the donor. Shape tests catch
  typos that break the format; only the owner's manual check catches a valid
  address that belongs to someone else.
- History becomes public with internal planning documents in it. They contain
  business planning, not credentials; the secret scan verifies the second half
  of that claim.
