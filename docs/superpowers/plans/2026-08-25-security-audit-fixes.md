# Security audit fixes (2026-08-25)

> Completed and merged (see git history). Checkboxes below were never ticked during execution; do not re-execute.

Closes the nine findings of the 2026-08-25 audit (main @ 328db30). One commit per
finding, smallest change first. Tests are written before the code they cover.

| Step | Finding | Change | Test |
|------|---------|--------|------|
| 1 | F1 CSV formula injection | `neutralize_formula` on the four text columns of `export_journal_csv`; `parse_journal_export` reverses it | unit table in `csv/export.rs`; integration in `tests/csv_import_export.rs` |
| 2 | F2 + N3 navigation, CSP | `nav_guard` plugin denies non-app origins for every webview; CSP gains `object-src 'none'`, `base-uri 'self'`, `form-action 'none'`, `frame-ancestors 'none'`, explicit `script-src`, and `blob:` for the viewer | `nav_guard` unit tests; config self-check |
| 3 | F9 dependency guard | `deny.toml` bans network crates on desktop targets; `cargo deny check` in CI; README sentence corrected | `cargo deny check` |
| 4 | F4 webview paths | Dialog-picked paths join the drop-path grant set; `vault_restore` / `csv_import_preview` accept only granted paths | `state.rs` and `commands.rs` unit tests |
| 5 | F5 SQLCipher memory security | `PRAGMA cipher_memory_security = ON` before `PRAGMA key` | `store.rs` unit test |
| 6 | F6 password zeroization | `Zeroizing<String>` command parameters | compile-time |
| 7 | F7 file modes | data dir `0700`, vault files and backups `0600` on Unix, fix-up on open | `permissions.rs`, `store.rs`, `backup.rs` tests (Unix) |
| 8 | F8 supply chain | Actions pinned to commit SHAs, `persist-credentials: false`, Dependabot | CI |
| 9 | F3 WebView2 installer | `bundle.windows.webviewInstallMode = offlineInstaller` | config self-check |

Out of scope: the macOS App Sandbox (moves the data directory; needs its own design note).
