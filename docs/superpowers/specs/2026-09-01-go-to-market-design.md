# Go-to-market readiness — design

Date: 2026-09-01
Status: approved (pending final user review)
Scope: everything between the current tree and a sellable macOS v1 under Ourovoros.io.

## Context

The 2026-09-01 product review found the engineering close to sellable and the
release/commerce lane not. Concretely: CI red on `main` since 2026-08-29
(`cargo deny` bans vs. the merged updater, two `webpki-roots` license
rejections, yanked `chacha20`, and a linker OOM in the `rust` job); an update
feed format (`latest.json.sig` + per-platform `sha256`) that nothing in
`release.yml` produces; releases pointed at a private repo GitHub will not
serve anonymously; no purchase path, EULA, or license-minting tool; a trial
resettable by deleting one JSON file; and first-run/localization polish gaps.

## Decisions (locked)

1. **Updater stays, click-driven.** The user-initiated update check ships in
   v1. The "no network" claim is rewritten as: offline by design; the single
   network action is the update check the user clicks.
2. **macOS-only first release.** Windows (signing stub) and Linux stay in the
   workflow as skeleton, unsold until signing lands.
3. **Paddle** is the sales channel (merchant of record; handles EU VAT).
   Fulfillment is manual in v1: order email, mint, send `.lic`.
4. **Public releases repo.** Artifacts + update feed live in
   `ourovoros-io/oikonomia-releases` (public). Source stays private.
5. **Source transfers to the org.** `GeorgiosDelkos/oikonomia` moves to
   `ourovoros-io/oikonomia` (private). Product identity follows the company.
6. **Light trial hardening.** Trial start stamped in prefs JSON and the macOS
   Keychain; earliest stamp wins.
7. **One program spec, one implementation plan**, five phases, each ending
   with a green full gate.

## Phase 0 — Company identity, repo transfer, policy truth

Goal: the repo tells the truth about itself and CI is green.

- **Repo transfer (user action, step one):** transfer
  `GeorgiosDelkos/oikonomia` to `ourovoros-io` (stays private). GitHub
  redirects the old path; the local remote is updated afterwards. No CI
  secrets exist yet, so nothing is lost. Create public
  `ourovoros-io/oikonomia-releases` with a README describing it as the
  binary-release home.
- **Identity rebrand:** bundle identifier `com.georgiosdelkos.oikonomia`
  becomes `io.ourovoros.oikonomia` everywhere (config, docs, tests that pin
  it). Consequence: the app-data directory moves; no shipped customers exist,
  so no migration code — the commit message records the old path for dev
  machines. `tauri.conf.json` bundle metadata filled in: publisher
  "Ourovoros.io", copyright "(c) 2026 Ourovoros.io", category
  `public.app-category.finance`, short/long descriptions,
  `minimumSystemVersion` "12.0". Workspace `Cargo.toml`: the incorrect
  `license = "MIT"` is dropped here; the `license-file = "EULA.md"` and
  `bundle.licenseFile` wiring land in Phase 2 with the file itself.
- **deny.toml scoped policy:** network-crate bans stay, with scoped
  exceptions (cargo-deny `wrappers`, or where wrapper chains get unwieldy, a
  tree-walk test) confining `ureq`/`rustls`/`webpki-roots` to the
  `oikonomia-update` path and `reqwest`/`hyper`/`tokio net` to
  `tauri-plugin-updater` in the desktop crate. The `tauri-plugin-updater` ban
  is removed (now intentional). `CDLA-Permissive-2.0` added to the license
  allowlist (Mozilla root-store data). `cargo update -p chacha20` clears the
  yanked advisory. Acceptance criteria, machine-checked: (a) a new Rust test
  asserts the `oikonomia-core` subtree contains zero network-capable crates;
  (b) any new network dependency outside the update path fails
  `cargo deny check`.
- **CI green:** set `CARGO_PROFILE_DEV_DEBUG` to `0` (or
  `line-tables-only`) for the CI build step to stop the linker OOM; add
  `npm run lint` (oxlint) to the web job; add a version-sync test asserting
  `tauri.conf.json` version equals the workspace version; delete the stray
  root `package-lock.json`.
- **Honesty rewrite:** README security section + threat model updated to the
  click-driven-updater story, including what the updater verifies (minisign
  over manifest and artifact) and the only host it talks to. Full feature
  rewrite waits for Phase 4.

## Phase 1 — Release lane

Goal: a signed, notarized macOS release whose update check succeeds.

- **Build (existing):** the `release.yml` macOS job keeps building signed,
  notarized artifacts into a draft release on the private repo, fail-closed
  on missing secrets, unchanged.
- **Promote (new):** a manually dispatched workflow, gated on the `release`
  GitHub environment with a required reviewer. Given a draft tag it:
  downloads artifacts; computes `sha256` per artifact; assembles
  `latest.json` itself with `darwin-aarch64` / `darwin-x86_64` entries each
  carrying `url` (pointing at the public repo's release assets), `signature`
  (the per-artifact minisign sig tauri produced), and `sha256`; signs
  `latest.json` with the same minisign key to produce `latest.json.sig`;
  publishes a release on `ourovoros-io/oikonomia-releases` carrying
  artifacts + feed + sig. We own the wire format end to end; the
  tauri-action shape mismatch disappears.
- **Updater crate:** feed URL const changes to
  `https://github.com/ourovoros-io/oikonomia-releases/releases/latest/download/latest.json`
  (host allowlist already covers github.com). New test: a fixture generated
  by the promote script's `--self-test` mode is checked in and must parse
  and verify through the real `perform_check_inner` path, so script and
  crate cannot drift silently. A `--dry-run` mode assembles and verifies the
  feed without publishing.
- **Smoke test:** `scripts/smoke-macos.sh` + `make smoke`: build with
  `cargo tauri build --bundles app`, launch the bundled `.app` (the bare
  binary is a known blank-window trap), capture the window with
  `screencapture -l`, fail on a near-white frame. Required in the macOS
  release job before a build is promotable.

## Phase 2 — Commerce

Goal: a customer can pay, receive a license, and the trial resists casual resets.

- **Minting CLI:** new crate `crates/oikonomia-mint`, depending on
  `oikonomia-core` so `signed_payload` stays the single source of truth.
  Subcommands: `keygen` (secret key file + printed public key), `issue
  --email --expiry` (emits `license.lic`), `verify`. The secret key lives
  only on the operator's machine. Tests: mint-to-verify round trip through
  core's real verifier, expiry edges, tamper rejection.
- **EULA:** `EULA.md` at repo root, proprietary terms naming Ourovoros.io,
  drafted in-repo and flagged for lawyer review before first sale. Wired via
  `Cargo.toml` `license-file = "EULA.md"`, `bundle.licenseFile`, and shown
  in-app (Settings About section, `include_str!`).
- **Buy path:** `license_status` gains a `buy_url` field served from a Rust
  const, `https://ourovoros.io/oikonomia` (real domain; no placeholder
  machinery — the go-live checklist covers the page being live). UI: Buy
  button in the Settings license section and in trial/expired states.
  Browser opening via `tauri-plugin-opener` with its capability scoped to
  exactly that URL.
- **Fulfillment runbook:** `docs/commerce.md` — Paddle account under
  Ourovoros.io, product setup, order email to mint to `.lic` email template,
  refund note. Webhook automation out of scope.
- **Trial hardening:** trial start recorded in prefs JSON and a macOS
  Keychain generic-password item (service `io.ourovoros.oikonomia`, via the
  `security-framework` crate); earliest stamp wins; Keychain-unavailable
  degrades to prefs-only. Logic tested against a `TrialStampStore` trait
  with a fake; a macOS-only integration test covers the real Keychain.

## Phase 3 — First-sale polish

- **Empty-state CTAs:** the five dead-end screens (Dashboard, Transactions,
  Accounts, Reports, Documents) pass an `action` to `EmptyState` opening the
  existing entity-create modal directly.
- **Trial visibility:** a slim global banner at 7 or fewer days remaining
  (always when expired) linking to the license section, localized.
- **Error localization:** shared `commandErrorMessage()` helper mapping the
  stable Rust error codes to localized strings; screens stop rendering raw
  English `message`. Guards: a vitest test that every code has a key in all
  four catalogs; a Rust test pinning the canonical code list to a shared
  fixture.
- **i18n parity guard:** vitest test that every `t()` key used in the app
  resolves in every locale; add the 4 missing FR and 2 DE translations.
- **Accessibility:** `role="alert"`/`aria-live` on `ErrorBanner`;
  `aria-invalid` + `aria-describedby` on the entry, unlock, and
  settings-password forms.

## Phase 4 — Round out

- **Account register screen:** drill-in from an account row on the Accounts
  page rendering the existing `account_register_cmd`; `api.ts` wrapper,
  page, tests.
- **README rewrite:** feature list matches reality (recurring, documents +
  OCR, updater, quick-add, licensing/trial, entry editing, opening balances,
  PDF export); stale claims gone.
- **Housekeeping:** `web/package.json` version set to the workspace version
  and covered by the version-sync test; the seven stale plan docs get a
  completed-header stamp.

## Out of scope (recorded, deliberate)

Windows/Linux go-live; Paddle webhook automation; macOS App Sandbox (still
the audit follow-up; would complicate Keychain and updater); balance-sheet /
trial-balance PDF export; JS bundle code-splitting; updater streaming
download (current in-memory cap is fine at this artifact size).

## Go-live checklist (user-only actions)

1. Transfer the repo to `ourovoros-io`; approve creation of
   `oikonomia-releases`.
2. Enroll Ourovoros.io in the Apple Developer Program (org enrollment needs
   a D-U-N-S number; individual enrollment would show a personal name in
   Gatekeeper prompts).
3. Create the `release` GitHub environment with yourself as required
   reviewer; enter `APPLE_*` and `TAURI_SIGNING_*` secrets per
   `docs/release.md`.
4. Run the minisign key ceremony (`keygen` for the license key; tauri signer
   for the updater key); bake both production public keys; store secrets
   offline.
5. Open the Paddle account under Ourovoros.io; put the checkout page live at
   `https://ourovoros.io/oikonomia`.
6. Have a lawyer review `EULA.md` before the first sale.
7. Bump the workspace version to `1.0.0` and run the first promote.

## Execution mechanics

One branch and PR per phase; TDD per house rules; full gate at each phase
end (`cargo fmt`, `clippy -D warnings`, `cargo deny check`, `cargo test`,
vitest, web build, and from Phase 1 on, `make smoke`). No emojis, no
Co-Authored-By trailers. Phases run 0 to 4 in order; Phase 0 cannot start
until the repo transfer is done.
