> **Superseded (2026-10-01).** Oikonomia is no longer sold. The commerce,
> licensing, and EULA parts of this document were removed by
> `docs/superpowers/specs/2026-10-01-open-source-donations-design.md`.
> The release lane, updater, and code signing described here still apply.

# Go-to-Market Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Take Oikonomia from a red-CI prototype to a sellable, signed, updatable macOS product owned by Ourovoros.io.

**Architecture:** Five phases, one branch + PR each, strictly in order. Phase 0 makes the repo tell the truth (identity, deny policy, CI green). Phase 1 makes releases deliverable (promote workflow producing the exact feed the updater parses, smoke test). Phase 2 makes it purchasable (mint CLI, EULA, buy link, trial hardening). Phases 3–4 polish first-run, localization, and round out the product.

**Tech Stack:** Rust (Tauri 2, ureq, minisign-verify, ed25519-dalek, security-framework), React/Vite/Tailwind, GitHub Actions, cargo-deny.

**Spec:** `docs/superpowers/specs/2026-09-01-go-to-market-design.md`

## Global Constraints

- Business logic lives in Rust (`oikonomia-core` / desktop crate), never in TypeScript (repo CLAUDE.md).
- Money is integer minor units `i64`; never `f64`.
- No emojis anywhere; no `Co-Authored-By` trailers in commits.
- Function names are full words; no cryptic abbreviations; rustdoc on `pub`/`pub(crate)` items.
- `rustfmt.toml` `use_small_heuristics = "Default"`; workspace clippy denies `unwrap_used`, `panic`, `print_stdout`, `print_stderr` (use `writeln!(io::stdout(), ...)` in CLIs), `allow_attributes` (use `#[expect]`).
- Every subagent must call `mcp__hippius-mem__recall` about its task before making changes and `mcp__hippius-mem__remember` durable gotchas it discovers.
- Bundle identifier after Phase 0: `io.ourovoros.oikonomia`. Company: Ourovoros.io. Buy URL: `https://ourovoros.io/oikonomia`. Public releases repo: `ourovoros-io/oikonomia-releases`.
- Phase gate (end of every phase): `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo deny check && cargo test --workspace && cd web && npx tsc -b && npm test && npm run lint && npm run build`.
- TDD: write the failing test first, watch it fail, implement, watch it pass, commit.

---

# Phase 0 — Identity, repo transfer, policy truth

Branch: `phase-0-identity-policy` (create after Task 1 completes).

### Task 1: Repo transfer and releases repo (USER-GATED)

The transfer must be confirmed by the repo owner. Everything else in the plan depends on it.

**Files:** none (GitHub + git remote only)

**Interfaces:**
- Produces: private repo `ourovoros-io/oikonomia`, public repo `ourovoros-io/oikonomia-releases`, local `origin` pointing at the new home.

- [ ] **Step 1: Ask the user to confirm, then transfer the source repo**

Present this command for the user to run themselves (or run it only after their explicit go-ahead in-session — it takes effect immediately):

```bash
gh api repos/GeorgiosDelkos/oikonomia/transfer -f new_owner=ourovoros-io
```

- [ ] **Step 2: Create the public releases repo**

```bash
gh repo create ourovoros-io/oikonomia-releases --public \
  --description "Binary releases and update feed for Oikonomia" \
  --clone=false
```

Then add a README via the API so the repo is not empty:

```bash
printf '%s\n' "# Oikonomia releases" "" \
  "Signed binary releases and the update feed (latest.json) for Oikonomia." \
  "Source code is developed in a private repository. Trust is the minisign" \
  "public key baked into the app, not this repository." > /tmp/releases-readme.md
gh api repos/ourovoros-io/oikonomia-releases/contents/README.md \
  -f message="docs: explain what this repo holds" \
  -f content="$(base64 < /tmp/releases-readme.md)"
```

- [ ] **Step 3: Update the local remote and verify**

```bash
git remote set-url origin git@github.com:ourovoros-io/oikonomia.git
git fetch origin && git status -sb
```

Expected: `## main...origin/main` with no divergence.

- [ ] **Step 4: Create the phase branch**

```bash
git checkout -b phase-0-identity-policy
```

### Task 2: Identity rebrand (bundle metadata + workspace manifest)

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `Cargo.toml` (workspace `[workspace.package]`)
- Modify: `apps/desktop/src-tauri/src/config_checks.rs`
- Modify: every non-historical reference to `com.georgiosdelkos.oikonomia` and `github.com/GeorgiosDelkos/oikonomia` (grep; skip `docs/superpowers/` history and this plan)

**Interfaces:**
- Produces: identifier `io.ourovoros.oikonomia` (used by Task 16's Keychain service name), bundle metadata pinned by tests.

- [ ] **Step 1: Write the failing tests** in `apps/desktop/src-tauri/src/config_checks.rs`:

```rust
#[test]
fn bundle_identity_belongs_to_ourovoros() {
    let conf = config();
    assert_eq!(conf["identifier"], "io.ourovoros.oikonomia");
    assert_eq!(conf["bundle"]["publisher"], "Ourovoros.io");
    assert_eq!(conf["bundle"]["copyright"], "Copyright (c) 2026 Ourovoros.io");
    // Tauri's bundle.category takes the shorthand name ("Finance"); if
    // `cargo tauri build --bundles app` rejects it, switch BOTH the config
    // and this assertion to "public.app-category.finance".
    assert_eq!(conf["bundle"]["category"], "Finance");
    assert_eq!(conf["bundle"]["macOS"]["minimumSystemVersion"], "12.0");
    assert!(
        conf["bundle"]["shortDescription"].as_str().is_some_and(|s| !s.is_empty()),
        "shortDescription must be set"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oikonomia bundle_identity` — expected FAIL (identifier is `com.georgiosdelkos.oikonomia`).

- [ ] **Step 3: Edit `tauri.conf.json`**: set `"identifier": "io.ourovoros.oikonomia"` and extend `bundle`:

```json
"bundle": {
  "active": true,
  "createUpdaterArtifacts": true,
  "targets": "all",
  "publisher": "Ourovoros.io",
  "copyright": "Copyright (c) 2026 Ourovoros.io",
  "category": "Finance",
  "shortDescription": "Encrypted double-entry books for people and small companies.",
  "longDescription": "Oikonomia keeps personal and company double-entry books in an encrypted local vault. Offline by design: the only network action is the update check you click.",
  "resources": ["resources/ocr/*"],
  "macOS": { "minimumSystemVersion": "12.0" },
  "windows": { "webviewInstallMode": { "type": "offlineInstaller" } },
  "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"]
}
```

- [ ] **Step 4: Edit workspace `Cargo.toml`**: delete the `license = "MIT"` line (Phase 2 adds `license-file`); set `repository = "https://github.com/ourovoros-io/oikonomia"`. Each crate inherits `license.workspace = true` — remove that line from all four crate manifests too (`crates/oikonomia-core`, `crates/oikonomia-update`, `crates/macos-dock-icon`, `apps/desktop/src-tauri`), or builds fail on the missing workspace key.

- [ ] **Step 5: Sweep remaining references**

Run: `grep -rn "com.georgiosdelkos\|GeorgiosDelkos/oikonomia" --include="*.rs" --include="*.json" --include="*.md" --include="*.yml" --include="*.toml" . | grep -v docs/superpowers | grep -v target | grep -v node_modules`

Update each hit (README.md:69 data-dir path, hosts.rs test URLs may wait for Task 9 but update now if trivial). Note in the commit message: the app-data dir moves from `com.georgiosdelkos.oikonomia` to `io.ourovoros.oikonomia`; dev machines keep their old vault at the old path.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p oikonomia && cargo test -p oikonomia-core` — expected PASS.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: rebrand product identity to Ourovoros.io

Identifier io.ourovoros.oikonomia, publisher/copyright/category/descriptions
pinned by config_checks. App-data dir moves; nothing shipped, dev vaults
stay at the old com.georgiosdelkos.oikonomia path."
```

### Task 3: Version-sync test and stray-file cleanup

**Files:**
- Modify: `apps/desktop/src-tauri/src/config_checks.rs`
- Modify: `web/package.json` (version `0.1.0`)
- Delete: `package-lock.json` (repo root, untracked empty stray)

**Interfaces:**
- Produces: a test that fails whenever `tauri.conf.json` version, workspace version, or `web/package.json` version drift.

- [ ] **Step 1: Write the failing test** in `config_checks.rs`:

```rust
#[test]
fn versions_are_in_sync_everywhere() {
    let conf = config();
    let workspace = env!("CARGO_PKG_VERSION");
    assert_eq!(conf["version"], workspace, "tauri.conf.json vs workspace");

    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../../../web/package.json"))
            .expect("web/package.json is valid JSON");
    assert_eq!(package["version"], workspace, "web/package.json vs workspace");
}
```

- [ ] **Step 2: Run to verify it fails** (`web/package.json` is `0.0.0`): `cargo test -p oikonomia versions_are_in_sync` — expected FAIL.

- [ ] **Step 3: Set `"version": "0.1.0"` in `web/package.json`** and run `cd web && npm install --package-lock-only` to refresh `web/package-lock.json`'s own version fields. Delete the stray root lockfile: `rm package-lock.json` (repo root; it is untracked and empty).

- [ ] **Step 4: Run the test** — expected PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "test: pin tauri.conf.json and web package version to the workspace version"
```

### Task 4: Scoped deny policy + advisories clean

**Files:**
- Modify: `deny.toml`
- Create: `scripts/assert-core-offline.sh`
- Modify: `Cargo.lock` (via `cargo update -p chacha20`)
- Modify: `Makefile` (`check` target runs the script)

**Interfaces:**
- Produces: `cargo deny check` green; `scripts/assert-core-offline.sh` exits nonzero if any network-capable crate enters `oikonomia-core`'s desktop subtree (Task 5 wires it into CI).

- [ ] **Step 1: Verify the current failure** (baseline): `cargo deny check 2>&1 | tail -3` — expected `advisories FAILED, bans FAILED, licenses FAILED`.

- [ ] **Step 2: Fix the yanked advisory**: `cargo update -p chacha20` then `cargo test -p oikonomia-core documents` (the OCR/PDF path still passes).

- [ ] **Step 3: Rewrite `deny.toml`.** Add to `[licenses].allow`: `"CDLA-Permissive-2.0"` (Mozilla root-store data, pulled by `webpki-roots`). Replace the `[bans].deny` list and `[bans].features` with:

```toml
[bans]
multiple-versions = "allow"
# Network crates are banned everywhere EXCEPT the user-initiated update path.
# `wrappers` = the only crates allowed to depend on the banned crate directly.
# Never widen a wrapper list for a non-updater parent; oikonomia-core stays
# fully offline (scripts/assert-core-offline.sh enforces the subtree).
deny = [
    { crate = "reqwest", wrappers = ["tauri-plugin-updater"], reason = "network only inside the updater install engine" },
    { crate = "ureq", wrappers = ["oikonomia-update"], reason = "network only inside the click-driven update client" },
    { crate = "isahc", reason = "offline except the update path" },
    { crate = "attohttpc", reason = "offline except the update path" },
    { crate = "minreq", reason = "offline except the update path" },
    { crate = "curl", reason = "offline except the update path" },
    { crate = "curl-sys", reason = "offline except the update path" },
    { crate = "hyper", wrappers = ["reqwest", "hyper-util", "hyper-rustls"], reason = "network only inside the updater install engine" },
    { crate = "hyper-util", wrappers = ["reqwest", "hyper-rustls"], reason = "network only inside the updater install engine" },
    { crate = "h2", wrappers = ["reqwest", "hyper"], reason = "network only inside the updater install engine" },
    { crate = "tungstenite", reason = "no websockets" },
    { crate = "tokio-tungstenite", reason = "no websockets" },
    { crate = "socket2", wrappers = ["tokio", "hyper-util"], reason = "sockets only under the updater engine" },
    { crate = "mio", wrappers = ["tokio"], reason = "sockets only under the updater engine" },
    { crate = "rustls", wrappers = ["ureq", "hyper-rustls", "tokio-rustls", "rustls-platform-verifier"], reason = "TLS only on the update path" },
    { crate = "native-tls", reason = "rustls only" },
    { crate = "tauri-plugin-http", reason = "the webview gets no HTTP" },
    { crate = "tauri-plugin-websocket", reason = "offline only" },
    { crate = "tauri-plugin-shell", reason = "the webview must not spawn processes" },
    { crate = "tauri-plugin-deep-link", reason = "no URL handlers" },
]
```

Notes: the `tauri-plugin-updater` ban and the `tokio` `net` feature ban are removed (both are now deliberate updater infrastructure); the `tauri-plugin-opener` ban is removed in Task 15 when the scoped capability lands — keep it for now:

```toml
    { crate = "tauri-plugin-opener", reason = "removed in phase 2 with a URL-scoped capability" },
```

- [ ] **Step 4: Iterate to green**: run `cargo deny check bans 2>&1 | head -40`. If a banned crate still errors, its printed parent chain names the direct parent; add that parent to the crate's `wrappers` ONLY if the chain roots in `tauri-plugin-updater`, `oikonomia-update`, or `tokio` under those two. Anything else is a real violation — stop and investigate. Repeat until `cargo deny check` prints `advisories ok, bans ok, licenses ok, sources ok`.

- [ ] **Step 5: Write `scripts/assert-core-offline.sh`** (mode 755):

```bash
#!/usr/bin/env bash
# oikonomia-core must stay fully offline: the scoped deny.toml wrappers allow
# network crates under the updater, so this guards the core subtree itself.
set -euo pipefail
cd "$(dirname "$0")/.."
banned='reqwest|ureq|isahc|attohttpc|minreq|curl|hyper|hyper-util|h2|tungstenite|socket2|mio|rustls|native-tls'
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  if cargo tree -p oikonomia-core -e normal --target "$target" --prefix none \
    | grep -E "^(${banned}) v"; then
    echo "error: network-capable crate inside oikonomia-core for ${target}" >&2
    exit 1
  fi
done
echo "oikonomia-core is offline on all desktop targets"
```

- [ ] **Step 6: Run both**: `./scripts/assert-core-offline.sh && cargo deny check` — expected both green. Add `./scripts/assert-core-offline.sh` as the line after `cargo deny check` in the Makefile `check` target.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: scope the network-crate ban to the update path

deny.toml wrappers confine ureq/rustls/reqwest to oikonomia-update and
tauri-plugin-updater; CDLA-Permissive-2.0 allowed for webpki-roots;
chacha20 un-yanked; assert-core-offline.sh keeps oikonomia-core's own
subtree at zero network crates on every desktop target."
```

### Task 5: CI back to green

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `scripts/assert-core-offline.sh` (Task 4).

- [ ] **Step 1: Edit `ci.yml`.** In the `rust` job, add a job-level env (linker on the runner dies with a bus error linking full debug info for the OCR+PDF+webkit binary):

```yaml
  rust:
    runs-on: ubuntu-latest
    env:
      CARGO_PROFILE_DEV_DEBUG: "line-tables-only"
```

In the `web` job add after `npx tsc -b`:

```yaml
      - run: npm run lint
```

In the `audit` job add after `cargo deny check`:

```yaml
      # The wrappers in deny.toml allow network crates under the updater;
      # core's own subtree must stay at zero.
      - run: ./scripts/assert-core-offline.sh
```

Also update the stale comment above `cargo deny check` (it still says "bans every crate that could open a socket") to: `# deny.toml confines network crates to the click-driven update path.`

- [ ] **Step 2: Verify locally what can be verified**: `cd web && npm run lint` passes; `cargo build -p oikonomia` still builds with `CARGO_PROFILE_DEV_DEBUG=line-tables-only cargo build -p oikonomia`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml && git commit -m "fix: unbreak CI - cap debug info for the linker, run oxlint and the core-offline guard"
```

### Task 6: README and threat-model honesty rewrite

**Files:**
- Modify: `README.md` (Security notes + Threat model sections only; feature list waits for Task 25)
- Modify: `AGENTS.md` (the "no network" invariant bullet)

- [ ] **Step 1: Rewrite `README.md` "Security notes"** — replace the network claims (current lines 70–77) with:

```markdown
- Offline by design: the app performs **no background network activity**. The
  single network action is the update check you click on the unlock screen; it
  talks only to `github.com` (the public `ourovoros-io/oikonomia-releases`
  repo) and verifies a minisign signature over both the update manifest and
  the downloaded artifact before anything is installed. The vault, ledger, and
  license paths (`oikonomia-core`) contain no network code at all —
  `scripts/assert-core-offline.sh` and `cargo deny check` enforce this in CI.
- Every webview is pinned to the app's own origin (`nav_guard`), and the CSP
  allows no remote source. The webview cannot supply a URL or key to the
  updater.
```

- [ ] **Step 2: Update the threat model** to add one row: "**Does not protect against:** a compromised GitHub account cannot ship a malicious update (artifacts are minisign-verified against the baked key), but a compromised signing key can — the key ceremony in docs/release.md keeps it offline."

- [ ] **Step 3: Update `AGENTS.md`** invariant bullet from "v1 Tauri capabilities: no network permission..." to: "Network exists ONLY on the click-driven update path (`oikonomia-update` + `tauri-plugin-updater` install engine). `oikonomia-core` stays fully offline (`scripts/assert-core-offline.sh`); `deny.toml` wrappers confine every socket-capable crate to that path."

- [ ] **Step 4: Commit**

```bash
git add README.md AGENTS.md && git commit -m "docs: replace the no-network claim with the honest click-driven-updater story"
```

### Task 7: Phase 0 gate and PR

- [ ] **Step 1: Run the full phase gate** (Global Constraints). All green, including `cargo deny check`.
- [ ] **Step 2: Push and open the PR**:

```bash
git push -u origin phase-0-identity-policy
gh pr create --title "Phase 0: company identity, scoped deny policy, CI green" \
  --body "Implements Phase 0 of docs/superpowers/specs/2026-09-01-go-to-market-design.md: Ourovoros.io identity + bundle metadata, version-sync test, scoped network policy (deny wrappers + core-offline guard), CI linker/oxlint fixes, honest README security story."
```

- [ ] **Step 3: Wait for CI on the PR to be green** (this is the first green CI since 2026-08-29 — confirm the `audit` and `rust` jobs specifically). Merge with the user's usual flow, then `git checkout main && git pull`.

---

# Phase 1 — Release lane

Branch: `phase-1-release-lane`.

### Task 8: Feed assembly in `oikonomia-update` (single source of truth)

**Files:**
- Create: `crates/oikonomia-update/src/feed.rs`
- Create: `crates/oikonomia-update/src/bin/assemble_feed.rs`
- Modify: `crates/oikonomia-update/src/lib.rs` (add `pub mod feed;` — `pub` so the bin uses it)

**Interfaces:**
- Produces: `feed::FeedArtifact { platform: String, file_name: String, signature: String, sha256_hex: String }`, `feed::assemble_manifest(version: &str, notes: &str, base_url: &str, artifacts: &[FeedArtifact]) -> crate::Result<String>`; bin `assemble_feed` with subcommands `assemble` and `verify` (used by Task 10's workflow and Task 9's tests).

- [ ] **Step 1: Write the failing test** in `feed.rs`:

```rust
#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{FeedArtifact, assemble_manifest};

    #[test]
    fn assembled_manifest_has_exactly_the_fields_the_client_parses() {
        let artifacts = vec![FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "Oikonomia_aarch64.app.tar.gz".to_owned(),
            signature: "TESTSIG".to_owned(),
            sha256_hex: "ab".repeat(32),
        }];
        let json = assemble_manifest(
            "1.0.0",
            "First release.",
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v1.0.0",
            &artifacts,
        )
        .expect("assemble");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["notes"], "First release.");
        let platform = &value["platforms"]["darwin-aarch64"];
        assert_eq!(
            platform["url"],
            "https://github.com/ourovoros-io/oikonomia-releases/releases/download/v1.0.0/Oikonomia_aarch64.app.tar.gz"
        );
        assert_eq!(platform["signature"], "TESTSIG");
        assert_eq!(platform["sha256"], "ab".repeat(32));
    }

    #[test]
    fn assemble_rejects_empty_inputs() {
        assert!(assemble_manifest("1.0.0", "", "https://example.com", &[]).is_err());
        let bad_sha = vec![FeedArtifact {
            platform: "darwin-aarch64".to_owned(),
            file_name: "a.tar.gz".to_owned(),
            signature: "SIG".to_owned(),
            sha256_hex: "zz".to_owned(),
        }];
        assert!(assemble_manifest("1.0.0", "", "https://example.com", &bad_sha).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**: `cargo test -p oikonomia-update feed` — FAIL (module missing).

- [ ] **Step 3: Implement `feed.rs`**:

```rust
//! Release-side manifest assembly.
//!
//! The promote workflow calls the `assemble_feed` bin, which calls
//! [`assemble_manifest`]; the client tests parse the same output through
//! `perform_check`, so the promote lane and the client cannot drift.

use crate::error::{Result, UpdateError};
use crate::verify::parse_sha256_hex;
use serde::Serialize;
use std::collections::BTreeMap;

/// One platform artifact entry destined for `latest.json`.
#[derive(Debug, Clone)]
pub struct FeedArtifact {
    /// Client platform key, e.g. `darwin-aarch64`.
    pub platform: String,
    /// File name of the artifact as uploaded to the release.
    pub file_name: String,
    /// Minisign signature over the artifact (contents of the `.sig` file).
    pub signature: String,
    /// Lowercase hex SHA-256 of the artifact bytes.
    pub sha256_hex: String,
}

#[derive(Serialize)]
struct ManifestPlatform {
    url: String,
    signature: String,
    sha256: String,
}

#[derive(Serialize)]
struct Manifest {
    version: String,
    notes: String,
    platforms: BTreeMap<String, ManifestPlatform>,
}

/// Build the exact `latest.json` body the update client parses.
///
/// # Errors
///
/// [`UpdateError::ManifestParse`] when there are no artifacts, or an entry has
/// an empty platform/file/signature or a malformed `sha256_hex`.
pub fn assemble_manifest(
    version: &str,
    notes: &str,
    base_url: &str,
    artifacts: &[FeedArtifact],
) -> Result<String> {
    if artifacts.is_empty() || version.trim().is_empty() {
        return Err(UpdateError::ManifestParse);
    }

    let base = base_url.trim_end_matches('/');
    let mut platforms = BTreeMap::new();
    for artifact in artifacts {
        if artifact.platform.trim().is_empty()
            || artifact.file_name.trim().is_empty()
            || artifact.signature.trim().is_empty()
        {
            return Err(UpdateError::ManifestParse);
        }
        parse_sha256_hex(&artifact.sha256_hex)?;
        platforms.insert(
            artifact.platform.clone(),
            ManifestPlatform {
                url: format!("{base}/{}", artifact.file_name),
                signature: artifact.signature.clone(),
                sha256: artifact.sha256_hex.to_lowercase(),
            },
        );
    }

    let manifest = Manifest {
        version: version.trim().trim_start_matches('v').to_owned(),
        notes: notes.to_owned(),
        platforms,
    };
    serde_json::to_string_pretty(&manifest).map_err(|_| UpdateError::ManifestParse)
}
```

Add `pub mod feed;` to `lib.rs` and re-export: `pub use feed::{FeedArtifact, assemble_manifest};`. If `parse_sha256_hex` is `pub(crate)` in `verify.rs`, that is already visible; do not widen it to `pub`.

- [ ] **Step 4: Run the tests** — PASS. Also `cargo clippy -p oikonomia-update --all-targets -- -D warnings`.

- [ ] **Step 5: Implement the bin** `src/bin/assemble_feed.rs`:

```rust
//! Promote-lane CLI: assemble and verify `latest.json`.
//!
//! `assemble` scans an artifact directory; `verify` checks a manifest and its
//! detached minisign signature with the client's own verifier code.

use oikonomia_update::{FeedArtifact, assemble_manifest, parse_public_key};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("assemble") => run_assemble(&args[1..]),
        Some("verify") => run_verify(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "{message}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:
  assemble_feed assemble --version <v> --base-url <url> --dir <artifact-dir> \
    --out <latest.json> [--notes-file <path>] --platform <key>=<file> [--platform ...]
  assemble_feed verify --manifest <latest.json> --sig <latest.json.sig> --pubkey <minisign-pubkey>";

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn flag_values(args: &[String], name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == name && index + 1 < args.len() {
            values.push(args[index + 1].clone());
            index += 2;
        } else {
            index += 1;
        }
    }
    values
}

fn run_assemble(args: &[String]) -> Result<(), String> {
    let version = flag_value(args, "--version").ok_or(USAGE)?;
    let base_url = flag_value(args, "--base-url").ok_or(USAGE)?;
    let dir = PathBuf::from(flag_value(args, "--dir").ok_or(USAGE)?);
    let out = PathBuf::from(flag_value(args, "--out").ok_or(USAGE)?);
    let notes = match flag_value(args, "--notes-file") {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("notes: {e}"))?,
        None => String::new(),
    };

    let mut artifacts = Vec::new();
    for mapping in flag_values(args, "--platform") {
        let (platform, file_name) = mapping
            .split_once('=')
            .ok_or_else(|| format!("bad --platform mapping: {mapping}"))?;
        let file = dir.join(file_name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let signature_path = dir.join(format!("{file_name}.sig"));
        let signature = std::fs::read_to_string(&signature_path)
            .map_err(|e| format!("{}: {e}", signature_path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256_hex = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        artifacts.push(FeedArtifact {
            platform: platform.to_owned(),
            file_name: file_name.to_owned(),
            signature: signature.trim().to_owned(),
            sha256_hex,
        });
    }

    let manifest = assemble_manifest(&version, notes.trim(), &base_url, &artifacts)
        .map_err(|e| format!("assemble: {e}"))?;
    std::fs::write(&out, manifest).map_err(|e| format!("{}: {e}", out.display()))?;
    writeln!(std::io::stdout(), "wrote {}", out.display()).map_err(|e| e.to_string())
}

fn run_verify(args: &[String]) -> Result<(), String> {
    let manifest = flag_value(args, "--manifest").ok_or(USAGE)?;
    let sig = flag_value(args, "--sig").ok_or(USAGE)?;
    let pubkey = flag_value(args, "--pubkey").ok_or(USAGE)?;

    let body = std::fs::read(&manifest).map_err(|e| format!("{manifest}: {e}"))?;
    let signature = std::fs::read_to_string(&sig).map_err(|e| format!("{sig}: {e}"))?;
    let key = parse_public_key(&pubkey).map_err(|e| format!("pubkey: {e}"))?;
    oikonomia_update::verify_manifest_bytes(&key, &body, &signature)
        .map_err(|e| format!("verify: {e}"))?;
    writeln!(std::io::stdout(), "manifest signature ok").map_err(|e| e.to_string())
}
```

This needs one small addition to the crate API — in `verify.rs` (or `lib.rs`) expose:

```rust
/// Verify a detached minisign signature over raw manifest bytes.
///
/// # Errors
///
/// [`UpdateError::ManifestSignature`] when verification fails.
pub fn verify_manifest_bytes(
    key: &minisign_verify::PublicKey,
    body: &[u8],
    signature: &str,
) -> Result<()> {
    verify_minisign(key, body, signature)
}
```

Export both from `lib.rs`. (`minisign_verify::PublicKey` is already a public type via `parse_public_key`.)

- [ ] **Step 6: Verify the bin compiles and clippy passes**: `cargo build -p oikonomia-update --bins && cargo clippy -p oikonomia-update --all-targets -- -D warnings`.

- [ ] **Step 7: Commit**

```bash
git add crates/oikonomia-update && git commit -m "feat: feed assembly module + assemble_feed bin

The promote workflow and the update client now share one manifest
implementation, closing the tauri-action shape mismatch (missing sha256,
missing manifest signature)."
```

### Task 9: Point the updater at the public repo + end-to-end feed test

**Files:**
- Modify: `crates/oikonomia-update/src/client.rs` (`UPDATE_FEED_URL`)
- Modify: `crates/oikonomia-update/src/hosts.rs` (test URLs)
- Modify: `crates/oikonomia-update/src/tests.rs` (new round-trip test)

**Interfaces:**
- Consumes: `feed::assemble_manifest` (Task 8); the existing test helpers in `tests.rs` (httptest server setup, minisign test keypair — reuse them, do not duplicate).

- [ ] **Step 1: Write the failing test** in `tests.rs` (adapt helper names to the ones already in that file — it already signs manifests with a `minisign` dev-dep keypair and serves them via `httptest`):

```rust
#[test]
fn promoted_feed_round_trips_through_check_and_download() {
    // Build the artifact and its minisign signature with the existing helpers.
    let artifact_bytes = b"new-app-bytes".to_vec();
    let (public_key, artifact_signature) = sign_with_test_key(&artifact_bytes);

    let sha256_hex = sha256_hex_of(&artifact_bytes);
    let manifest = oikonomia_update::assemble_manifest(
        "9.9.9",
        "Promoted release.",
        &server_base_url(),
        &[oikonomia_update::FeedArtifact {
            platform: test_platform(),
            file_name: "Oikonomia_test.app.tar.gz".to_owned(),
            signature: artifact_signature,
            sha256_hex,
        }],
    )
    .expect("assemble");

    // Serve latest.json, latest.json.sig (signed manifest bytes), artifact.
    let outcome = check_against_served_feed(&manifest, &artifact_bytes, &public_key);
    let CheckOutcome::Available(offer) = outcome else {
        panic!("promoted feed must yield Available, got {outcome:?}");
    };
    let path = download_and_verify(&test_config(&public_key), &offer).expect("download");
    assert!(path.exists());
}
```

The exact helper names differ — read `tests.rs` first and reuse its existing httptest + minisign scaffolding (`ClientConfig::for_test`, `HostPolicy::test_http_hosts`). The essential assertions: `assemble_manifest` output passes `perform_check` to `Available` and `download_and_verify` succeeds.

- [ ] **Step 2: Run to verify it fails** only for the right reason (compiles, then assertion churn as you wire helpers): `cargo test -p oikonomia-update promoted_feed` — get it to PASS by wiring, since the production code already exists. If it fails on manifest parsing, `assemble_manifest` and `RawManifest` have drifted — fix `feed.rs`, never the test.

- [ ] **Step 3: Change the feed URL** in `client.rs`:

```rust
/// GitHub Releases CDN for `latest.json` on the public releases repo.
/// Trust is the baked minisign key, not GitHub.
pub const UPDATE_FEED_URL: &str =
    "https://github.com/ourovoros-io/oikonomia-releases/releases/latest/download/latest.json";
```

Update the two `hosts.rs` test URLs from `GeorgiosDelkos/oikonomia` to `ourovoros-io/oikonomia-releases`.

- [ ] **Step 4: Run the full crate suite**: `cargo test -p oikonomia-update` — PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/oikonomia-update && git commit -m "feat: point the update feed at ourovoros-io/oikonomia-releases and prove the promoted feed round-trips"
```

### Task 10: Promote workflow

**Files:**
- Create: `.github/workflows/promote.yml`
- Modify: `docs/release.md`

**Interfaces:**
- Consumes: `assemble_feed` bin (Task 8); draft releases produced by `release.yml`; new secret `RELEASES_REPO_TOKEN` (fine-grained PAT with contents:write on `ourovoros-io/oikonomia-releases` — go-live checklist).

- [ ] **Step 1: Write `.github/workflows/promote.yml`**:

```yaml
# Promote a draft release from this private repo to the public releases repo.
#
# Manual, reviewed, fail-closed. Steps: download the draft's artifacts,
# assemble latest.json ourselves (sha256 + per-artifact minisign sigs),
# sign latest.json with the updater minisign key, verify with the app's
# baked public key, then publish everything to ourovoros-io/oikonomia-releases.
#
# Needs (Environment `release`):
#   TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#   RELEASES_REPO_TOKEN  fine-grained PAT, contents:write on the public repo
name: Promote release

on:
  workflow_dispatch:
    inputs:
      tag:
        description: Tag of the draft release to promote (e.g. v1.0.0)
        required: true
        type: string
      dry_run:
        description: Assemble, sign, and verify the feed without publishing
        type: boolean
        default: false

permissions:
  contents: read

jobs:
  promote:
    runs-on: ubuntu-latest
    environment: release
    env:
      TAG: ${{ inputs.tag }}
      PUBLIC_REPO: ourovoros-io/oikonomia-releases
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
        with:
          persist-credentials: false

      - name: Require promote secrets (fail-closed)
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          RELEASES_REPO_TOKEN: ${{ secrets.RELEASES_REPO_TOKEN }}
        run: |
          missing=()
          [ -z "${TAURI_SIGNING_PRIVATE_KEY}" ] && missing+=(TAURI_SIGNING_PRIVATE_KEY)
          [ -z "${RELEASES_REPO_TOKEN}" ] && missing+=(RELEASES_REPO_TOKEN)
          if [ "${#missing[@]}" -gt 0 ]; then
            echo "::error::Missing required secrets: ${missing[*]}"
            exit 1
          fi

      - uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772 # v1
        with:
          toolchain: stable

      - uses: Swatinem/rust-cache@49a0bdc70d2e1b713ca9e2869b211fcce03d3c1c # v2, 2026-08-25

      # gh resolves draft releases by tag when authenticated with repo access.
      # If this ever stops matching the draft, list releases via
      # `gh api repos/${GITHUB_REPOSITORY}/releases` and download assets by id.
      - name: Download draft release artifacts
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          mkdir -p artifacts
          gh release download "${TAG}" --repo "${GITHUB_REPOSITORY}" --dir artifacts
          ls -la artifacts

      - name: Assemble latest.json (darwin only in v1)
        run: |
          set -euo pipefail
          aarch64="$(basename "$(ls artifacts/*aarch64*.app.tar.gz)")"
          x64="$(basename "$(ls artifacts/*x64*.app.tar.gz 2>/dev/null || true)")"
          platforms=(--platform "darwin-aarch64=${aarch64}")
          [ -n "${x64}" ] && platforms+=(--platform "darwin-x86_64=${x64}")
          printf '%s' "Oikonomia ${TAG}" > notes.txt
          cargo run -p oikonomia-update --bin assemble_feed -- assemble \
            --version "${TAG}" \
            --base-url "https://github.com/${PUBLIC_REPO}/releases/download/${TAG}" \
            --dir artifacts --out artifacts/latest.json --notes-file notes.txt \
            "${platforms[@]}"

      - name: Sign latest.json with the updater key
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: |
          extra=()
          [ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ] \
            && extra=(--password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD}")
          npx --yes @tauri-apps/cli signer sign \
            --private-key "${TAURI_SIGNING_PRIVATE_KEY}" \
            "${extra[@]}" artifacts/latest.json
          test -s artifacts/latest.json.sig

      - name: Verify the signed manifest with the app's baked public key
        run: |
          pubkey="$(python3 -c 'import json;print(json.load(open("apps/desktop/src-tauri/tauri.conf.json"))["plugins"]["updater"]["pubkey"])')"
          cargo run -p oikonomia-update --bin assemble_feed -- verify \
            --manifest artifacts/latest.json --sig artifacts/latest.json.sig \
            --pubkey "${pubkey}"

      - name: Publish to the public releases repo
        if: ${{ inputs.dry_run != true }}
        env:
          GH_TOKEN: ${{ secrets.RELEASES_REPO_TOKEN }}
        run: |
          gh release create "${TAG}" artifacts/* \
            --repo "${PUBLIC_REPO}" \
            --title "Oikonomia ${TAG}" \
            --notes "Signed release. The app verifies the minisign signature of latest.json and of every artifact before installing."
```

- [ ] **Step 2: Validate the YAML**: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/promote.yml'))"` (PyYAML is available on the runner; locally `npx --yes yaml-lint` works too — any parse check is fine).

- [ ] **Step 3: Update `docs/release.md`**: add a "Promote" section — tag push builds drafts in the private repo; `gh workflow run promote.yml -f tag=vX.Y.Z` (requires environment review) publishes to `ourovoros-io/oikonomia-releases`; add `RELEASES_REPO_TOKEN` to the secret list; note the updater reads ONLY the public repo.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/promote.yml docs/release.md && git commit -m "feat: promote workflow publishes signed feed + artifacts to the public releases repo"
```

### Task 11: macOS smoke test

**Files:**
- Create: `scripts/smoke-macos.sh` (mode 755)
- Create: `scripts/smoke-window-check.swift`
- Modify: `Makefile` (add `smoke` target)
- Modify: `.github/workflows/release.yml` (macOS job: smoke after build)

**Interfaces:**
- Produces: `make smoke` — builds the bundle, launches the `.app`, fails if the window renders (near-)blank. Known gotcha: the bare `cargo build` binary shows a blank white window even on healthy code; ONLY the bundled `.app` is meaningful.

- [ ] **Step 1: Write `scripts/smoke-window-check.swift`**:

```swift
// Finds the frontmost window owned by "Oikonomia", captures it, and exits
// nonzero when the capture is (near-)uniformly white - the blank-webview
// failure the bare binary always shows and a broken bundle can show.
import CoreGraphics
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

let windowList = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID)
    as? [[String: Any]] ?? []
guard let window = windowList.first(where: {
    ($0[kCGWindowOwnerName as String] as? String) == "Oikonomia"
        && (($0[kCGWindowBounds as String] as? [String: Any])?["Width"] as? Double ?? 0) > 300
}), let windowID = window[kCGWindowNumber as String] as? CGWindowID else {
    fail("no Oikonomia window found")
}

guard let image = CGWindowListCreateImage(
    .null, .optionIncludingWindow, windowID, [.boundsIgnoreFraming]
) else {
    fail("could not capture window (grant Screen Recording to the terminal)")
}

guard let data = image.dataProvider?.data as Data?, image.bitsPerPixel == 32 else {
    fail("unexpected pixel format")
}

var nonWhite = 0
var total = 0
let bytesPerRow = image.bytesPerRow
for y in stride(from: 0, to: image.height, by: 8) {
    for x in stride(from: 0, to: image.width, by: 8) {
        let offset = y * bytesPerRow + x * 4
        let r = data[offset], g = data[offset + 1], b = data[offset + 2]
        total += 1
        if r < 240 || g < 240 || b < 240 { nonWhite += 1 }
    }
}
let ratio = total == 0 ? 0 : Double(nonWhite) / Double(total)
print("non-white pixel ratio: \(ratio)")
if ratio < 0.05 {
    fail("window is (near-)blank - the UI did not render")
}
```

- [ ] **Step 2: Write `scripts/smoke-macos.sh`**:

```bash
#!/usr/bin/env bash
# Smoke-test the BUNDLED app. The bare cargo binary renders a blank window
# even on healthy code, so only the .app bundle proves anything.
set -euo pipefail
cd "$(dirname "$0")/.."

app="target/release/bundle/macos/Oikonomia.app"
if [ "${SMOKE_SKIP_BUILD:-0}" != "1" ]; then
  cargo tauri build --bundles app
fi
[ -d "$app" ] || { echo "error: $app missing" >&2; exit 1; }

open "$app"
trap 'osascript -e "tell application \"Oikonomia\" to quit" >/dev/null 2>&1 || true' EXIT
sleep 10

swift scripts/smoke-window-check.swift
echo "smoke ok: the bundled app rendered a real window"
```

`chmod 755 scripts/smoke-macos.sh`. Add to `Makefile`:

```make
# Build the .app bundle and verify it renders (never smoke the bare binary).
smoke:
	./scripts/smoke-macos.sh
```

- [ ] **Step 3: Run it locally**: `make smoke`. Expected: the app opens, the script prints a non-white ratio well above 0.05, exits 0. First run may prompt for Screen Recording permission — grant it to the terminal and rerun.

- [ ] **Step 4: Wire into `release.yml`** macOS job, immediately after the tauri-action step:

```yaml
      - name: Smoke-test the bundled app
        run: SMOKE_SKIP_BUILD=1 ./scripts/smoke-macos.sh
```

(tauri-action has already produced `target/release/bundle/macos/Oikonomia.app`; a blank render fails the job before anyone can promote the draft.)

- [ ] **Step 5: Commit**

```bash
git add scripts Makefile .github/workflows/release.yml && git commit -m "feat: bundled-app smoke test gates the release lane

Captures the real window and fails on a near-blank frame; the bare binary
is never smoked (known blank-window trap)."
```

### Task 12: Phase 1 gate and PR

- [ ] **Step 1: Run the phase gate** plus `make smoke`.
- [ ] **Step 2: Push, open PR** titled "Phase 1: release lane — promoted feed, public releases repo, smoke test", wait for green CI, merge, return to `main`.

---

# Phase 2 — Commerce

Branch: `phase-2-commerce`.

### Task 13: `oikonomia-mint` CLI crate

**Files:**
- Create: `crates/oikonomia-mint/Cargo.toml`, `crates/oikonomia-mint/src/main.rs`
- Modify: `Cargo.toml` (workspace members; NOT `default-members`)

**Interfaces:**
- Consumes: `oikonomia_core::license::{signed_payload, install_license, LicenseVerifier, PRODUCT}`.
- Produces: `oikonomia-mint keygen --out <secret-key-file>`; `oikonomia-mint issue --key <file> --email <e> --expiry <YYYY-MM-DD> --out <license.lic>`; `oikonomia-mint verify --lic <file> [--pubkey-hex <64-hex>]`.

- [ ] **Step 1: Create the crate.** `Cargo.toml`:

```toml
[package]
name = "oikonomia-mint"
version.workspace = true
edition.workspace = true
repository.workspace = true
rust-version.workspace = true
description = "Offline license minting for Oikonomia. Run only on an operator machine; the secret key never leaves it."

[dependencies]
oikonomia-core = { workspace = true }
ed25519-dalek = { workspace = true }
rand = { workspace = true }
serde_json = { workspace = true }
time = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }

[lints]
workspace = true
```

Add `"crates/oikonomia-mint"` to workspace `members`. Check `ed25519-dalek` workspace dep has the `rand_core` feature for key generation; if `SigningKey::generate` is missing, add `features = ["rand_core"]` to the workspace `ed25519-dalek` entry.

- [ ] **Step 2: Write the failing tests** in `main.rs`:

```rust
#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use super::{generate_secret_key_hex, mint_license_json, verifying_key_hex};
    use oikonomia_core::license::{
        LicenseState, LicenseVerifier, install_license, license_status,
    };

    fn hex_to_key(hex: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex");
        }
        out
    }

    #[test]
    fn minted_license_installs_and_reports_licensed() {
        let secret_hex = generate_secret_key_hex();
        let public_hex = verifying_key_hex(&secret_hex).expect("pubkey");
        let json = mint_license_json(&secret_hex, "buyer@example.com", "2999-12-31")
            .expect("mint");

        let dir = tempfile::tempdir().expect("tempdir");
        let lic_path = dir.path().join("bought.lic");
        std::fs::write(&lic_path, &json).expect("write");

        let verifier =
            LicenseVerifier::from_public_key_bytes(&hex_to_key(&public_hex)).expect("verifier");
        let status = install_license(dir.path(), &lic_path, &verifier).expect("install");
        assert_eq!(status.state, LicenseState::Licensed);
        assert_eq!(status.licensed_until.as_deref(), Some("2999-12-31"));
        let again = license_status(dir.path(), &verifier).expect("status");
        assert_eq!(again.state, LicenseState::Licensed);
    }

    #[test]
    fn tampered_license_is_rejected() {
        let secret_hex = generate_secret_key_hex();
        let public_hex = verifying_key_hex(&secret_hex).expect("pubkey");
        let json = mint_license_json(&secret_hex, "buyer@example.com", "2999-12-31")
            .expect("mint");
        let tampered = json.replace("buyer@example.com", "thief@example.com");

        let dir = tempfile::tempdir().expect("tempdir");
        let lic_path = dir.path().join("tampered.lic");
        std::fs::write(&lic_path, tampered).expect("write");
        let verifier =
            LicenseVerifier::from_public_key_bytes(&hex_to_key(&public_hex)).expect("verifier");
        assert!(install_license(dir.path(), &lic_path, &verifier).is_err());
    }

    #[test]
    fn expiry_must_be_a_calendar_date() {
        let secret_hex = generate_secret_key_hex();
        assert!(mint_license_json(&secret_hex, "b@example.com", "not-a-date").is_err());
        assert!(mint_license_json(&secret_hex, "", "2999-12-31").is_err());
    }
}
```

- [ ] **Step 3: Run to verify it fails**: `cargo test -p oikonomia-mint` — FAIL (functions missing).

- [ ] **Step 4: Implement** `main.rs` (library functions + thin CLI):

```rust
//! Offline license minting. The Ed25519 secret key lives ONLY on the
//! operator's machine; this binary is never shipped, never in CI.

use ed25519_dalek::{Signer, SigningKey};
use oikonomia_core::license::signed_payload;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Random 32-byte Ed25519 seed as lowercase hex.
fn generate_secret_key_hex() -> String {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    hex_encode(key.as_bytes())
}

/// Hex verifying key for a hex secret key (what gets baked into the app).
fn verifying_key_hex(secret_hex: &str) -> Result<String, String> {
    let key = signing_key_from_hex(secret_hex)?;
    Ok(hex_encode(key.verifying_key().as_bytes()))
}

/// Build the signed `.lic` JSON for one buyer.
fn mint_license_json(secret_hex: &str, email: &str, expiry: &str) -> Result<String, String> {
    if email.trim().is_empty() || !email.contains('@') {
        return Err("email must be a real address".to_owned());
    }
    let date_format = time::macros::format_description!("[year]-[month]-[day]");
    if expiry.len() != 10 || time::Date::parse(expiry, &date_format).is_err() {
        return Err(format!("expiry must be YYYY-MM-DD, got {expiry}"));
    }

    let issued_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| e.to_string())?;
    let key = signing_key_from_hex(secret_hex)?;
    let payload = signed_payload(oikonomia_core::license::PRODUCT, expiry, email, &issued_at);
    let signature = key.sign(payload.as_bytes());

    let json = serde_json::json!({
        "v": 1,
        "product": oikonomia_core::license::PRODUCT,
        "expiry": expiry,
        "email": email,
        "issued_at": issued_at,
        "sig": hex_encode(&signature.to_bytes()),
    });
    serde_json::to_string_pretty(&json).map_err(|e| e.to_string())
}

fn signing_key_from_hex(secret_hex: &str) -> Result<SigningKey, String> {
    let trimmed = secret_hex.trim();
    if trimmed.len() != 64 {
        return Err("secret key must be 64 hex characters".to_owned());
    }
    let mut seed = [0u8; 32];
    for (i, slot) in seed.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&trimmed[i * 2..i * 2 + 2], 16)
            .map_err(|_| "secret key is not hex".to_owned())?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
```

Then the CLI `main` with the same std-args style as `assemble_feed` (`flag_value` helper): `keygen` writes the secret hex to `--out` with permissions 0o600 (`std::os::unix::fs::PermissionsExt`, behind `#[cfg(unix)]`) and prints the PUBLIC key hex to stdout with the message "bake this into license.rs PRODUCTION_PUBLIC_KEY_HEX"; `issue` reads `--key`, calls `mint_license_json`, writes `--out`; `verify` reads `--lic` and verifies with `--pubkey-hex` (default: `oikonomia_core::license::PRODUCTION_PUBLIC_KEY_HEX`) via `LicenseVerifier::from_public_key_bytes` + `install_license` into a temp dir. Use `writeln!(std::io::stdout(), ...)` (the `print_stdout` lint denies `println!`).

- [ ] **Step 5: Run tests**: `cargo test -p oikonomia-mint && cargo clippy -p oikonomia-mint --all-targets -- -D warnings` — PASS.

- [ ] **Step 6: Manual round trip** (keygen prints the public key hex on stdout — capture it):

```bash
scratch="$(mktemp -d)"
public_key_hex="$(cargo run -q -p oikonomia-mint -- keygen --out "$scratch/mint.key" | tail -1)"
cargo run -q -p oikonomia-mint -- issue --key "$scratch/mint.key" \
  --email test@example.com --expiry 2027-01-01 --out "$scratch/test.lic"
cargo run -q -p oikonomia-mint -- verify --lic "$scratch/test.lic" \
  --pubkey-hex "$public_key_hex"
rm -rf "$scratch"
```

(Make keygen's LAST stdout line exactly the bare public key hex so the capture above works; the human-readable "bake this" hint goes to the line before it. Confirm `assert-core-offline.sh` and `cargo deny check` still pass — mint adds no network crates.)

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: oikonomia-mint offline license CLI

keygen/issue/verify against oikonomia-core's own signed_payload and
install_license, so the mint lane and the app verifier cannot drift."
```

### Task 14: EULA

**Files:**
- Create: `EULA.md`
- Modify: `Cargo.toml` (workspace: `license-file = "EULA.md"`) and all four crate manifests (`license-file.workspace = true`)
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (`bundle.licenseFile`)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`eula_text` command), `apps/desktop/src-tauri/src/lib.rs` (register)
- Modify: `web/src/lib/api.ts`, `web/src/pages/SettingsPage.tsx`, all four `web/src/locales/*.json`

**Interfaces:**
- Produces: IPC `eula_text() -> String`; Settings shows a "License agreement" viewer.

- [ ] **Step 1: Write `EULA.md`** — a plain-language proprietary EULA: parties (Ourovoros.io, single-legal-entity seller, Greece), grant (personal, non-transferable, per-person license; trial terms), restrictions (no redistribution/resale of license files), ownership, no warranty / limitation of liability to the price paid, termination, governing law Greece/EU, contact `https://ourovoros.io/oikonomia`. Head the file with: `<!-- DRAFT - requires review by counsel before first sale (go-live checklist item 6). -->`

- [ ] **Step 2: Wire the manifests**: workspace `Cargo.toml` `[workspace.package]` gets `license-file = "EULA.md"`; each crate manifest gets `license-file.workspace = true` (this replaces the `license` keys deleted in Task 2). `tauri.conf.json` bundle gains `"licenseFile": "../../../EULA.md"` (path relative to the tauri crate). `cargo build -p oikonomia-core` to confirm cargo accepts it.

- [ ] **Step 3: Failing test for the IPC command** (in `commands.rs` test module):

```rust
#[test]
fn eula_text_is_bundled_and_nonempty() {
    let text = super::eula_text_content();
    assert!(text.contains("Ourovoros.io"));
    assert!(text.len() > 1000, "EULA suspiciously short");
}
```

- [ ] **Step 4: Implement** in `commands.rs`:

```rust
/// The bundled end-user license agreement (EULA.md at the repo root).
pub(crate) fn eula_text_content() -> &'static str {
    include_str!("../../../../EULA.md")
}

/// Return the EULA for the Settings "About" section.
#[tauri::command]
pub fn eula_text() -> String {
    eula_text_content().to_owned()
}
```

Register `commands::eula_text` in `lib.rs` `ipc_commands()`. Run the test — PASS.

- [ ] **Step 5: Web viewer.** `api.ts` gains:

```ts
export async function eulaText(): Promise<string> {
  if (!isTauri()) return ''
  return invoke<string>('eula_text')
}
```

In `SettingsPage.tsx` license section (after the import-license controls) add a link-style button `t('settings.license.viewEula')` that opens a modal (reuse the existing dialog pattern + `useDialogFocus`) rendering the EULA text in a scrollable `<pre className="whitespace-pre-wrap">`. i18n keys in all four catalogs: `settings.license.viewEula` = "License agreement" / "Άδεια χρήσης" / "Contrat de licence" / "Lizenzvereinbarung"; `settings.license.eulaTitle` same values. Add a vitest in `SettingsPage.test.tsx`: clicking the button shows the mocked EULA text.

- [ ] **Step 6: Run the suites**: `cargo test -p oikonomia && cd web && npm test` — PASS.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: proprietary EULA bundled, shown in-app, and set as the workspace license-file"
```

### Task 15: Buy path (buy_url + scoped opener)

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs` (license_status payload), `apps/desktop/src-tauri/Cargo.toml` (+ `tauri-plugin-opener`), `apps/desktop/src-tauri/src/lib.rs` (plugin), `apps/desktop/src-tauri/capabilities/default.json`, `apps/desktop/src-tauri/src/config_checks.rs`
- Modify: `deny.toml` (drop the `tauri-plugin-opener` ban, reason documented)
- Modify: `web/package.json` (+ `@tauri-apps/plugin-opener`), `web/src/lib/license.ts`, `web/src/lib/api.ts`, `web/src/pages/SettingsPage.tsx`, locales

**Interfaces:**
- Produces: `license_status` IPC now returns `{ ...LicenseStatus, buy_url: string }`; const `BUY_URL: &str = "https://ourovoros.io/oikonomia"`; capability `opener:allow-open-url` scoped to exactly that URL prefix.

- [ ] **Step 1: Failing Rust test** (`commands.rs` tests):

```rust
#[test]
fn license_status_payload_carries_the_buy_url() {
    let payload = super::LicenseStatusPayload {
        status: oikonomia_core::license::LicenseStatus {
            state: oikonomia_core::license::LicenseState::None,
            days_remaining: None,
            licensed_until: None,
        },
        buy_url: super::BUY_URL.to_owned(),
    };
    let json = serde_json::to_value(&payload).expect("serialize");
    assert_eq!(json["state"], "none");
    assert_eq!(json["buy_url"], "https://ourovoros.io/oikonomia");
}
```

- [ ] **Step 2: Implement**: in `commands.rs`:

```rust
/// Where a customer buys a license. The page carries the Paddle checkout;
/// the app itself never talks to it - the browser does.
pub(crate) const BUY_URL: &str = "https://ourovoros.io/oikonomia";

/// `license_status` IPC payload: core status plus the buy link.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct LicenseStatusPayload {
    #[serde(flatten)]
    pub(crate) status: LicenseStatus,
    pub(crate) buy_url: String,
}
```

Change the `license_status` command's return type to `CommandResult<LicenseStatusPayload>` wrapping the existing status (and `license_install`'s `Option<LicenseStatus>` equally — wrap as `Option<LicenseStatusPayload>`). Run tests — PASS.

- [ ] **Step 3: Opener plugin**: `apps/desktop/src-tauri/Cargo.toml` add `tauri-plugin-opener = "2"`; in `lib.rs` `with_desktop_plugins` add `.plugin(tauri_plugin_opener::init())` with the comment `// URL opening is capability-scoped to the buy page only.`; remove the `tauri-plugin-opener` ban from `deny.toml` (leave the shell/deep-link bans); `capabilities/default.json` permissions gain:

```json
    {
      "identifier": "opener:allow-open-url",
      "allow": [{ "url": "https://ourovoros.io/oikonomia*" }]
    }
```

- [ ] **Step 4: Pin the scope with a test** in `config_checks.rs` (new — capabilities file):

```rust
#[test]
fn opener_capability_is_scoped_to_the_buy_page_only() {
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capabilities json");
    let entries: Vec<&serde_json::Value> = capabilities["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .filter(|p| p["identifier"] == "opener:allow-open-url")
        .collect();
    assert_eq!(entries.len(), 1, "exactly one opener permission");
    let allow = entries[0]["allow"].as_array().expect("allow list");
    assert_eq!(allow.len(), 1);
    assert_eq!(allow[0]["url"], "https://ourovoros.io/oikonomia*");
}
```

- [ ] **Step 5: Web**: `cd web && npm install @tauri-apps/plugin-opener`. `license.ts`: `LicenseStatus` gains `buy_url?: string`. `api.ts` (or wherever `licenseStatus()` lives — follow the existing wrapper) passes it through. In `SettingsPage.tsx` license section add a primary Buy button when `state !== 'licensed'`:

```tsx
{license && license.state !== 'licensed' && license.buy_url ? (
  <Button
    onClick={() => {
      void openUrl(license.buy_url ?? '')
    }}
  >
    {t('settings.license.buy')}
  </Button>
) : null}
```

with `import { openUrl } from '@tauri-apps/plugin-opener'`. i18n keys ×4: `settings.license.buy` = "Buy a license" / "Αγορά άδειας" / "Acheter une licence" / "Lizenz kaufen"; and extend `settings.license.description` to mention where: EN "Import a signed license file. Buy one at ourovoros.io/oikonomia. Nothing is sent from this computer." (mirror in EL/FR/DE). Vitest: mock `@tauri-apps/plugin-opener`, assert the button renders for trial/expired and calls `openUrl` with the `buy_url` from the mocked status.

- [ ] **Step 6: Run everything**: `cargo test -p oikonomia && cargo deny check && cd web && npm test` — PASS.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: buy path - license_status carries buy_url, opener scoped to the buy page

tauri-plugin-opener un-banned deliberately; the capability allows exactly
https://ourovoros.io/oikonomia* and a config test pins that scope."
```

### Task 16: Trial hardening (second stamp in the macOS Keychain)

**Files:**
- Modify: `crates/oikonomia-core/src/license.rs` (store trait + `_with` functions)
- Create: `apps/desktop/src-tauri/src/trial_store.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs` (use the store), `apps/desktop/src-tauri/src/lib.rs` (`mod trial_store;`), `apps/desktop/src-tauri/Cargo.toml` (`security-framework` under macOS)

**Interfaces:**
- Produces (core): `pub trait TrialStampStore { fn read_stamp(&self) -> Option<String>; fn write_stamp(&self, rfc3339: &str); }`, `pub struct NoTrialStampStore;`, `pub fn record_trial_start_with(data_dir: &Path, store: &dyn TrialStampStore) -> Result<()>`, `pub fn license_status_at_with(data_dir: &Path, verifier: &LicenseVerifier, now: OffsetDateTime, store: &dyn TrialStampStore) -> LicenseStatus`, `pub fn license_status_with(data_dir, verifier, store) -> Result<LicenseStatus>`, `pub fn require_writes_allowed_with(data_dir, verifier, store) -> Result<()>`.
- Produces (desktop): `trial_store::default_trial_store() -> &'static dyn TrialStampStore` (macOS Keychain service `io.ourovoros.oikonomia`, account `trial-started-at`; no-op elsewhere).
- Existing `record_trial_start` / `license_status_at` keep their signatures and delegate with `NoTrialStampStore` (tests stay valid).

- [ ] **Step 1: Failing core tests** (in `license.rs` tests or `tests/license_flow.rs`, matching where trial tests live today):

```rust
struct FakeStampStore(std::sync::Mutex<Option<String>>);

impl TrialStampStore for FakeStampStore {
    fn read_stamp(&self) -> Option<String> {
        self.0.lock().ok().and_then(|guard| guard.clone())
    }
    fn write_stamp(&self, rfc3339: &str) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(rfc3339.to_owned());
        }
    }
}

#[test]
fn deleting_the_prefs_stamp_does_not_reset_the_trial() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FakeStampStore(std::sync::Mutex::new(None));
    record_trial_start_with(dir.path(), &store).expect("stamp");
    let stamped = store.read_stamp().expect("secondary stamp written");

    // Casual reset: user deletes ui-prefs.json.
    std::fs::remove_file(crate::prefs::ui_prefs_path(dir.path())).expect("delete prefs");

    // Re-stamping must restore the ORIGINAL date from the secondary store.
    record_trial_start_with(dir.path(), &store).expect("re-stamp");
    let prefs = crate::prefs::load_ui_prefs(dir.path());
    assert_eq!(prefs.trial_started_at.as_deref(), Some(stamped.as_str()));
}

#[test]
fn earliest_stamp_wins_for_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let verifier = test_verifier(); // reuse the file's existing ephemeral-key helper
    let old = "2020-01-01T00:00:00Z";
    let store = FakeStampStore(std::sync::Mutex::new(Some(old.to_owned())));

    // Prefs say "today", keychain says 2020: trial is long over.
    record_trial_start_with(dir.path(), &NoTrialStampStore).expect("prefs stamp");
    let now = OffsetDateTime::now_utc();
    let status = license_status_at_with(dir.path(), &verifier, now, &store);
    assert_eq!(status.state, LicenseState::Expired);
}
```

- [ ] **Step 2: Run to verify failure**: `cargo test -p oikonomia-core license` — FAIL (trait missing).

- [ ] **Step 3: Implement in core.** Trait + no-op store as in Interfaces. `record_trial_start_with`: read prefs stamp and store stamp; the earliest parseable RFC3339 of the two (or the current time if neither) becomes the canonical stamp; write it to BOTH (prefs always; store via `write_stamp`, ignoring store failures — the store is best-effort by contract). `license_status_at_with`: same as `license_status_at` but the trial branch uses the earliest of the two stamps; an unparseable stamp in either place is treated as expired (as today). Rewrite `record_trial_start` / `license_status_at` / `license_status` / `require_writes_allowed` as thin delegations passing `&NoTrialStampStore`. Run core tests — PASS (including all pre-existing license tests, unchanged).

- [ ] **Step 4: Desktop store.** `apps/desktop/src-tauri/Cargo.toml`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
security-framework = "3"
```

`src/trial_store.rs`:

```rust
//! Secondary trial stamp. Deleting the app-data dir must not reset the trial.

use oikonomia_core::license::TrialStampStore;

/// Keychain generic password: service = bundle identifier, account = stamp.
#[cfg(target_os = "macos")]
pub struct MacKeychainTrialStore;

#[cfg(target_os = "macos")]
const SERVICE: &str = "io.ourovoros.oikonomia";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "trial-started-at";

#[cfg(target_os = "macos")]
impl TrialStampStore for MacKeychainTrialStore {
    fn read_stamp(&self) -> Option<String> {
        security_framework::passwords::get_generic_password(SERVICE, ACCOUNT)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    fn write_stamp(&self, rfc3339: &str) {
        if let Err(err) = security_framework::passwords::set_generic_password(
            SERVICE,
            ACCOUNT,
            rfc3339.as_bytes(),
        ) {
            log::warn!("keychain trial stamp write failed: {err}");
        }
    }
}

/// The platform's best secondary stamp store.
#[must_use]
pub fn default_trial_store() -> &'static dyn TrialStampStore {
    #[cfg(target_os = "macos")]
    {
        static STORE: MacKeychainTrialStore = MacKeychainTrialStore;
        &STORE
    }
    #[cfg(not(target_os = "macos"))]
    {
        static STORE: oikonomia_core::license::NoTrialStampStore =
            oikonomia_core::license::NoTrialStampStore;
        &STORE
    }
}
```

- [ ] **Step 5: Wire the desktop call sites**: `stamp_trial_start` calls `record_trial_start_with(state.data_dir(), trial_store::default_trial_store())`; `require_writes` and the `license_status` / `license_install` commands use the `_with` variants with the same store. Add a macOS-only integration test marked `#[ignore = "touches the real login keychain"]` that round-trips a stamp and then deletes it via `security_framework::passwords::delete_generic_password`; document running it manually with `cargo test -p oikonomia --ignored keychain`.

- [ ] **Step 6: Full workspace tests**: `cargo test --workspace && cargo clippy --all-targets --all-features -- -D warnings` — PASS.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat: trial stamp survives app-data deletion via a second macOS Keychain stamp

Earliest stamp wins; keychain is best-effort so tests and CI (and Linux/
Windows) degrade to the prefs stamp unchanged."
```

### Task 17: Commerce runbook

**Files:**
- Create: `docs/commerce.md`
- Modify: `docs/release.md` (key ceremony gains the license keypair)

- [ ] **Step 1: Write `docs/commerce.md`**: Paddle account under Ourovoros.io (merchant of record — Paddle invoices the buyer and handles EU VAT); one product ("Oikonomia license", 1-year expiry date convention `issue date + 1 year` unless the user decides perpetual — record the decision here at go-live); checkout lives at `https://ourovoros.io/oikonomia`. Fulfillment (manual v1): Paddle order email arrives → on the operator machine run `cargo run -p oikonomia-mint -- issue --key <offline-key-path> --email <buyer> --expiry <date> --out license.lic` → send the reply using the template below → archive the order id and expiry in your records (never commit buyer PII). Include the email template (short: thanks, attach `license.lic`, "Settings → License → Import license", link to the EULA, refund policy pointer). State explicitly: the license secret key never goes to GitHub, CI, Paddle, or any server; webhook automation is out of scope for v1.

- [ ] **Step 2: Update `docs/release.md`** key ceremony: two keypairs — updater minisign (`npx @tauri-apps/cli signer generate`, private → Environment `release`, public → `tauri.conf.json` + `update_key.rs`) and license Ed25519 (`oikonomia-mint keygen` on the operator machine, private stays offline, public hex → `license.rs PRODUCTION_PUBLIC_KEY_HEX`); note that the CURRENT baked license key predates any ceremony and MUST be regenerated before first sale (go-live checklist item 4).

- [ ] **Step 3: Commit**

```bash
git add docs && git commit -m "docs: Paddle fulfillment runbook and the two-key ceremony"
```

### Task 18: Phase 2 gate and PR

- [ ] **Step 1: Phase gate** (all commands from Global Constraints) plus `make smoke`.
- [ ] **Step 2: PR** "Phase 2: commerce — mint CLI, EULA, buy path, trial hardening"; green CI; merge; back to `main`.

---

# Phase 3 — First-sale polish

Branch: `phase-3-polish`.

### Task 19: Localized command errors

**Files:**
- Create: `web/src/lib/commandError.ts`, `web/src/lib/commandError.test.ts`, `web/src/lib/errorCodes.json`
- Modify: `apps/desktop/src-tauri/src/error.rs` (test pinning the code list)
- Modify: all four locale files; every page that renders `cmd.message` raw (`AccountsPage.tsx`, `ReportsPage.tsx`, `DocumentsPage.tsx`, `RecurringPage.tsx`, `TransactionsPage.tsx`, `App.tsx`)

**Interfaces:**
- Produces: `commandErrorMessage(err: CommandError): string` — localized copy for a known `code`, else the English `message`; `errorCodes.json` = the canonical array of codes, pinned on both sides.

- [ ] **Step 1: Create `web/src/lib/errorCodes.json`** with every code `From<CoreError>` emits (desktop `error.rs`):

```json
["vault_uninitialized", "vault_locked", "invalid_password", "unbalanced_entry",
 "too_few_lines", "invalid_line_amounts", "account_wrong_entity", "money_overflow",
 "negative_money", "validation", "io", "crypto", "vault_corrupt", "backup_invalid",
 "restore_would_overwrite", "not_found", "analysis", "csv_parse",
 "license_invalid", "license_expired", "license_entity_limit", "unknown"]
```

- [ ] **Step 2: Failing Rust test** in `error.rs`:

```rust
#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail loudly by design")]
mod tests {
    use oikonomia_core::Error as CoreError;

    #[test]
    fn every_emitted_code_is_in_the_shared_fixture() {
        let fixture: Vec<String> =
            serde_json::from_str(include_str!("../../../../web/src/lib/errorCodes.json"))
                .expect("errorCodes.json");
        let samples: Vec<CoreError> = vec![
            CoreError::VaultUninitialized,
            CoreError::VaultLocked,
            CoreError::InvalidPassword,
            CoreError::TooFewLines,
            CoreError::InvalidLineAmounts,
            CoreError::AccountWrongEntity,
            CoreError::MoneyOverflow,
            CoreError::NegativeMoney,
            CoreError::Validation("x".into()),
            CoreError::Io("x".into()),
            CoreError::Crypto("x".into()),
            CoreError::VaultCorrupt("x".into()),
            CoreError::BackupInvalid("x".into()),
            CoreError::RestoreWouldOverwrite,
            CoreError::NotFound("x".into()),
            CoreError::Analysis("x".into()),
            CoreError::CsvParse("x".into()),
            CoreError::LicenseInvalid,
            CoreError::LicenseExpired,
            CoreError::LicenseEntityLimit,
        ];
        for sample in samples {
            let code = super::CommandError::from(sample).code;
            assert!(fixture.contains(&code), "code {code} missing from errorCodes.json");
        }
    }
}
```

(`UnbalancedEntry` has fields — construct it too if its field types are simple; otherwise assert the literal `"unbalanced_entry"` is in the fixture.) Run: FAIL until the fixture file exists; then PASS.

- [ ] **Step 3: Failing vitest** `commandError.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import codes from './errorCodes.json'
import { commandErrorMessage, ERROR_CODE_KEYS } from './commandError'
import { flattenMessages } from './i18n'
import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

describe('command error localization', () => {
  it('maps every canonical code in every locale', () => {
    const catalogs = { en, el, fr, de }
    for (const code of codes) {
      const key = ERROR_CODE_KEYS[code]
      expect(key, `no i18n key for code ${code}`).toBeTruthy()
      for (const [locale, catalog] of Object.entries(catalogs)) {
        const flat = flattenMessages(catalog)
        expect(flat[key], `${locale} missing ${key}`).toBeTruthy()
      }
    }
  })

  it('falls back to the raw message for unknown codes', () => {
    expect(commandErrorMessage({ code: 'brand_new', message: 'raw text' })).toBe('raw text')
  })
})
```

- [ ] **Step 4: Implement `commandError.ts`**:

```ts
import { t } from './i18n'
import type { CommandError } from './tauri'

/** Canonical code -> i18n key. Pinned by errorCodes.json on both sides. */
export const ERROR_CODE_KEYS: Record<string, string> = {
  vault_uninitialized: 'error.vaultUninitialized',
  vault_locked: 'error.vaultLocked',
  invalid_password: 'error.invalidPassword',
  unbalanced_entry: 'error.unbalancedEntry',
  too_few_lines: 'error.tooFewLines',
  invalid_line_amounts: 'error.invalidLineAmounts',
  account_wrong_entity: 'error.accountWrongEntity',
  money_overflow: 'error.moneyOverflow',
  negative_money: 'error.negativeMoney',
  validation: 'error.validation',
  io: 'error.io',
  crypto: 'error.crypto',
  vault_corrupt: 'error.vaultCorrupt',
  backup_invalid: 'error.backupInvalid',
  restore_would_overwrite: 'error.restoreWouldOverwrite',
  not_found: 'error.notFound',
  analysis: 'error.analysis',
  csv_parse: 'error.csvParse',
  license_invalid: 'error.licenseInvalid',
  license_expired: 'error.licenseExpired',
  license_entity_limit: 'error.licenseEntityLimit',
  unknown: 'error.unknown',
}

/** Localized copy for a known command error code; raw English otherwise. */
export function commandErrorMessage(err: CommandError): string {
  const key = ERROR_CODE_KEYS[err.code]
  if (key) {
    const copy = t(key)
    if (copy !== key) return copy
  }
  return err.message
}
```

Add the 22 `error.*` strings to `en.json` (flat) and nested `error: {...}` objects to `el/fr/de.json` — plain, user-facing phrasings (e.g. `error.unbalancedEntry` EN "Debits and credits do not balance."; `error.validation` EN "That input is not valid."; `error.io` EN "A file operation failed."). Run the vitest — PASS.

- [ ] **Step 5: Swap the call sites.** In each listed page, replace raw `cmd.message` / `err.message` renders with `commandErrorMessage(cmd)` (keep the existing license/backup/PDF special-case helpers — they run FIRST, `commandErrorMessage` is the general fallback, exactly the `UnlockScreen` branching pattern). Run `npm test` — the pages' existing tests must stay green; update test expectations that asserted raw English messages.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: localize command errors via the shared code fixture

Rust pins its emitted codes to errorCodes.json; the web maps every code
to catalog copy in all four locales with the raw message as fallback."
```

### Task 20: Empty-state CTAs

**Files:**
- Modify: `web/src/App.tsx`, `web/src/pages/DashboardPage.tsx` (:139), `web/src/pages/TransactionsPage.tsx` (:550), `web/src/pages/AccountsPage.tsx` (:173), `web/src/pages/ReportsPage.tsx` (:109), `web/src/pages/DocumentsPage.tsx` (:81), `web/src/pages/SettingsPage.tsx`, locales, tests

**Interfaces:**
- Produces: App-level `onCreateBook: () => void` prop threaded to the five pages; `SettingsPage` prop `createBookIntent?: number` (increment = open the create-book form and scroll to it).

- [ ] **Step 1: Failing test** (e.g. in `DashboardPage.test.tsx` + one for App wiring): with no entity, the empty state renders a button labeled `t('empty.createBook')`; clicking it calls the `onCreateBook` prop.

- [ ] **Step 2: Implement.** In `App.tsx`: `const [createBookIntent, setCreateBookIntent] = useState(0)` and `const openCreateBook = useCallback(() => { setActive('settings'); setCreateBookIntent((n) => n + 1) }, [])`; pass `onCreateBook={openCreateBook}` to the five pages and `createBookIntent={createBookIntent}` to `SettingsPage`. In `SettingsPage`, a `useEffect` on `createBookIntent > 0` opens the existing create-entity form (whatever state currently gates it at the section around line 666) and scrolls it into view (`ref.scrollIntoView({ behavior: 'smooth' })`). In each of the five pages, pass to the no-book `EmptyState`:

```tsx
action={
  onCreateBook ? (
    <Button onClick={onCreateBook}>{t('empty.createBook')}</Button>
  ) : undefined
}
```

New key ×4: `empty.createBook` = "Create a book" / "Δημιουργία βιβλίου" / "Créer un livre" / "Buch erstellen".

- [ ] **Step 3: Run `npm test`** — all green (update snapshots/queries the five pages' tests need).

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: first-run empty states lead straight to creating a book"
```

### Task 21: Global trial banner

**Files:**
- Create: `web/src/components/TrialBanner.tsx`, `web/src/components/TrialBanner.test.tsx`
- Modify: `web/src/App.tsx` (mount above the page content when unlocked), locales

**Interfaces:**
- Consumes: `licenseStatus()` wrapper (already fetched in SettingsPage — App fetches once on unlock), `buy_url` (Task 15), `openUrl`.
- Produces: banner shown when `state === 'trial' && days_remaining <= 7`, or always when `state === 'expired'`.

- [ ] **Step 1: Failing component test**: renders nothing for `{state:'trial', days_remaining: 20}`; renders countdown copy for `days_remaining: 3`; renders expired copy + Buy button for `{state:'expired', buy_url}`; Buy click calls the mocked `openUrl`.

- [ ] **Step 2: Implement `TrialBanner.tsx`**:

```tsx
import { openUrl } from '@tauri-apps/plugin-opener'
import { t } from '../lib/i18n'
import type { LicenseStatus } from '../lib/license'
import { Button } from './ui'

const SHOW_AT_DAYS = 7

export function TrialBanner({ license }: { license: LicenseStatus | null }) {
  if (!license) return null
  const expiring =
    license.state === 'trial' &&
    license.days_remaining != null &&
    license.days_remaining <= SHOW_AT_DAYS
  const expired = license.state === 'expired'
  if (!expiring && !expired) return null

  return (
    <div
      role="status"
      className="flex items-center justify-between gap-3 border-b border-[var(--color-border-strong)] bg-[var(--color-surface-elevated)] px-4 py-2 text-sm"
    >
      <span>
        {expired
          ? t('trial.banner.expired')
          : t('trial.banner.expiring', { n: license.days_remaining ?? 0 })}
      </span>
      {license.buy_url ? (
        <Button onClick={() => void openUrl(license.buy_url ?? '')}>
          {t('settings.license.buy')}
        </Button>
      ) : null}
    </div>
  )
}
```

Keys ×4: `trial.banner.expiring` = "{n} days left in your trial." (+ EL/FR/DE), `trial.banner.expired` = "Trial ended. Your books stay readable; buy a license to keep writing." (+ EL/FR/DE). In `App.tsx`, fetch the license status where entities are loaded after unlock, keep it in state, render `<TrialBanner license={license} />` above the active page, and refresh it after `license_install` succeeds (listen the same way SettingsPage updates it, or lift SettingsPage's status up — smallest diff wins).

- [ ] **Step 3: `npm test`** green.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: trial countdown and expiry banner are visible outside Settings"
```

### Task 22: Accessibility + locale gap-fill + parity guard

**Files:**
- Modify: `web/src/components/ui.tsx` (ErrorBanner), `web/src/components/DateInput.tsx` neighbors as needed, `web/src/pages/UnlockScreen.tsx` + entry + settings-password forms (aria wiring), `web/src/locales/fr.json` (4 strings), `web/src/locales/de.json` (2 strings), `web/src/lib/i18n.ts` (+ export), `web/src/lib/i18n.test.ts`

- [ ] **Step 1: ErrorBanner announcement** — add to the outer div in `ErrorBanner`: `role="alert"` and `aria-live="assertive"`. Update/add a `ui` test asserting the role.

- [ ] **Step 2: Form aria wiring** — in the unlock form, simple-entry form, and settings password form: inputs with a visible error get `aria-invalid={true}` and `aria-describedby` pointing at the error element's `id`. Add one test per form (Testing Library `getByRole('textbox', ...)` + `toHaveAccessibleDescription`).

- [ ] **Step 3: Translate the leftovers** — `fr.json`: `tx.csv.field.date`, `tx.csv.field.description`, `tx.csv.col.type`, `tx.csv.col.note` ("Date", "Description" stay identical only if genuinely correct French — use "Date", "Description", "Type", "Note" with proper French casing); `de.json`: `date.month.11` ("Nov."), `recurring.form.name` ("Name" is correct German — if so, mark resolved by asserting intent in the test below instead).

- [ ] **Step 4: Parity guard.** In `i18n.ts` export a test-facing helper:

```ts
/** Test-only: true when `key` resolves in `locale` without the en fallback. */
export function resolvesInLocale(locale: Locale, key: string): boolean {
  return lookup(locale, key) !== undefined
}
```

New test in `i18n.test.ts`:

```ts
it('every english catalog key resolves in every locale', () => {
  const englishKeys = Object.keys(flattenMessages(en))
  for (const locale of ['el', 'fr', 'de'] as const) {
    const missing = englishKeys.filter((key) => !resolvesInLocale(locale, key))
    expect(missing, `${locale} silently falls back for: ${missing.join(', ')}`).toEqual([])
  }
})
```

Run it — it fails listing any real gaps; fill those keys in the writer catalogs (through `KEY_ALIASES` where the nested path differs) until green.

- [ ] **Step 5: `npm test` + `npm run lint`** green.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "fix: screen-reader announcements, form aria wiring, locale parity guard"
```

### Task 23: Phase 3 gate and PR

- [ ] Phase gate; PR "Phase 3: first-sale polish — localized errors, CTAs, trial banner, a11y"; green CI; merge; back to `main`.

---

# Phase 4 — Round out

Branch: `phase-4-roundout`.

### Task 24: Account register screen

**Files:**
- Modify: `web/src/lib/api.ts` (wrapper + type), `web/src/pages/AccountsPage.tsx` (drill-in view)
- Create: `web/src/pages/AccountRegister.test.tsx` (or extend `AccountsPage.test.tsx` — follow the existing test-file layout)
- Locales ×4

**Interfaces:**
- Consumes: IPC `account_register_cmd(account_id, from?, to?) -> Vec<RegisterLine>` (already registered; `RegisterLine` = `{ entry_id, entry_date, description, debit_minor, credit_minor, balance_minor, hidden }`).
- Produces: `api.accountRegister(accountId: string, from?: string, to?: string): Promise<RegisterLine[]>`.

- [ ] **Step 1: Failing api + component test**: mock invoke; clicking an account row shows a register table with date, description, debit, credit, running balance columns (money via the existing minor-units formatter), hidden rows visually muted, and a back button restoring the list.

- [ ] **Step 2: Implement.** `api.ts`:

```ts
export type RegisterLine = {
  entry_id: string
  entry_date: string
  description: string
  debit_minor: number
  credit_minor: number
  balance_minor: number
  hidden: boolean
}

export async function accountRegister(
  accountId: string,
  from?: string,
  to?: string,
): Promise<RegisterLine[]> {
  // Copy the invoke + error-normalization shape of the neighboring wrappers
  // in this file verbatim (try/catch rethrowing the normalized CommandError):
  return invoke<RegisterLine[]>('account_register_cmd', { accountId, from, to })
}
```

(Read `web/src/lib/api.ts` first and follow its existing wrapper pattern exactly — including however it normalizes errors; do not invent a new helper.) In `AccountsPage.tsx`: `const [registerAccount, setRegisterAccount] = useState<Account | null>(null)`; row click (or a "Register" row action button, whichever the table structure supports cleanly) sets it; when set, render the register panel instead of the list; load lines on mount, `ErrorBanner` + `commandErrorMessage` on failure; keys ×4: `accounts.register.title` ("{name} register"), `accounts.register.back` ("All accounts"), `accounts.register.balance` ("Balance"), `accounts.register.empty` ("No posted entries for this account yet.").

- [ ] **Step 3: `npm test`** green.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "feat: per-account register view over the existing account_register command"
```

### Task 25: README feature rewrite + docs hygiene

**Files:**
- Modify: `README.md`, the seven `docs/superpowers/plans/2026-08-*.md` files

- [ ] **Step 1: Rewrite the README "v1 features" list** to match reality — add: recurring transactions, document capture with offline OCR (bundled models), signed click-driven update check, tray quick-add window, entry editing, opening balances, P&L PDF export, offline licensing with a 30-day trial, EULA, buy link. Remove "Recurring transactions" from the backlog list; the backlog keeps: attachments, budgets, invoicing, multi-currency, recovery key, Windows/Linux go-live, webhook fulfillment, App Sandbox. Update the Layout table (add `crates/oikonomia-update`, `crates/oikonomia-mint`, `crates/macos-dock-icon`). Update the Develop section (`make check`, `make smoke`).

- [ ] **Step 2: Stamp the stale plans** — prepend to each 2026-08 plan doc under its title: `> Completed and merged (see git history). Checkboxes below were never ticked during execution; do not re-execute.`

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/plans && git commit -m "docs: README matches the shipped product; stale plan docs stamped completed"
```

### Task 26: Phase 4 gate, PR, and handoff

- [ ] **Step 1: Full phase gate** + `make smoke`.
- [ ] **Step 2: PR** "Phase 4: account register, README truth, docs hygiene"; green CI; merge.
- [ ] **Step 3: Report the remaining go-live checklist** to the user (spec section "Go-live checklist"): Apple enrollment (D-U-N-S), `release` environment + `APPLE_*`/`TAURI_SIGNING_*`/`RELEASES_REPO_TOKEN` secrets, both key ceremonies (updater + license — the current baked license key MUST be regenerated), Paddle account + checkout page live at `https://ourovoros.io/oikonomia`, lawyer review of `EULA.md`, then bump the workspace version to `1.0.0`, tag `v1.0.0`, and run the promote workflow.
