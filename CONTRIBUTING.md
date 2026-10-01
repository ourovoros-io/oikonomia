# Contributing to Oikonomia

Thanks for helping. Oikonomia is a local-only, encrypted, double-entry finance
app. Changes are judged first on whether they keep those three properties.

## Setup

- Rust stable 1.94 or newer, Node 22 or newer
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

```bash
cargo tauri dev          # run the desktop app
make check               # local quality gate (fmt, clippy, deny, tests, web build)
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

## License

By contributing you agree that your contribution is licensed under
GPL-3.0-or-later, the same license as the project.

## Security issues

Do not open a public issue. See [SECURITY.md](SECURITY.md).
