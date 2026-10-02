# Contributing to Oikonomia

Thanks for helping. Oikonomia is a local-only, encrypted, double-entry finance
app. Changes are judged first on whether they keep those three properties.

## Setup

- Rust stable (the workspace declares 1.94 as its minimum; CI tracks the latest stable)
- Node 22.22.2 or newer on 22.x, 24.15 or newer, or 26 or newer
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- Tauri CLI: `cargo install tauri-cli --version "^2" --locked`
- cargo-deny: `cargo install cargo-deny --locked`
- Run `cd web && npm ci && npm run build` once (or `mkdir -p web/dist`) so the
  Tauri shell crate compiles; workspace-wide `cargo` commands fail without
  `web/dist`.

```bash
cargo tauri dev          # run the desktop app
make check               # local gate: fmt check, clippy, deny, offline-core check, core tests, web tsc/test/build
```

`make check` is not all of CI. CI also runs `cargo test --workspace`,
`cargo build -p oikonomia`, `npm run lint`, and `cargo audit`.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
cd web && npx tsc -b && npm run lint && npm test
```

Behavior changes need tests. Keep commits small and explain why in the
message.

## Translations

UI strings live in `web/src/locales`: `en.json` has flat keys, and `el.json`,
`fr.json` and `de.json` are nested. The alias table in `web/src/lib/i18n.ts`
(`KEY_ALIASES`) maps each flat English key to its nested path. Every key must
have a reader in production code and exist in all four languages with the same
placeholders; `web/src/lib/i18n.catalog.test.ts` enforces this.

To add a backend error or analyzer note:

1. Add the variant and its `snake_case` code in Rust (`ValidationError`,
   `DesktopError`, `UpdateError`, `UiTextCode`, `AnalyzerHint` or
   `SyntheticLine`), and add it to the enumeration's `ALL` list (or
   `ALL_CODES`). Each of these lists is guarded by a test that lists the
   variants once in a `listed_variants!` call: the tests do not compile until
   the new variant is in that call, and they fail until it is also in the
   enumeration's `ALL` list.
2. Add the code to the matching JSON file in `web/src/lib` (`errorCodes.json`
   or `uiTextCodes.json`), and to `errorCodeParams.json` if it has parameters.
3. Map the code to a catalog key in `web/src/lib/commandError.ts` or
   `web/src/lib/uiText.ts`, and write the copy in all four languages.

Text that is written into a book (seeded account names, generated
descriptions) belongs in the table in `crates/oikonomia-core/src/text.rs`, not
in the catalogs. The UI never renders raw backend text.

## Rules that are not negotiable

- Money is integer minor units (`i64`). Never a float.
- Every posted journal entry balances: debits equal credits, at least two lines.
- Business rules live in `oikonomia-core`, never in TypeScript.
- `oikonomia-core` has no network code. The only network path is the
  click-driven update check in `oikonomia-update`.
- The vault is encrypted at rest. No plaintext database on disk.
- No new dependency without a reason a small local implementation cannot meet.
  New dependencies must be permissively licensed or MPL-2.0; `cargo deny check`
  enforces it.

`AGENTS.md` lists the full set of invariants.

## License

By contributing you agree that your contribution is licensed under
GPL-3.0-or-later, the same license as the project.

## Security issues

Do not open a public issue. See [SECURITY.md](SECURITY.md).
