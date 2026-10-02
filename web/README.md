# Oikonomia web UI

The React UI that the Tauri shell renders inside the Oikonomia desktop app. It
holds no business logic: money rules, ledger rules, and encryption live in the
Rust core (`crates/oikonomia-core`) and reach the UI through Tauri commands.

## Scripts

Run these from this directory.

```bash
npm install                  # or npm ci for a clean install
npm run dev                  # Vite dev server (use `cargo tauri dev` from the repo root for the full app)
npm run build                # type-check, then production build into dist/
npm run lint                 # oxlint
npm test                     # vitest
npm run preview              # serve the production build locally
npm run capture:marketing    # marketing screenshots (see marketing/README.md)
```

See [CONTRIBUTING.md](../CONTRIBUTING.md) in the repository root for setup,
the full quality gate, and the project rules.
