# Cutting a desktop release

Ops note for the tag-gated desktop workflow. Not a product README.

## GitHub Environment `release`

Workflow YAML cannot create Environments.

1. GitHub → Settings → Environments → New environment.
2. Name it exactly `release`.
3. Add required reviewers.
4. After the Apple Developer account (individual team, no App Store) exists, add only the secrets named at the top of [`.github/workflows/release.yml`](../.github/workflows/release.yml).

Required secrets:

- The Apple set: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY` (or `APPLE_API_KEY_ID`) and `APPLE_API_KEY_P8`.
- The updater key: `TAURI_SIGNING_PRIVATE_KEY` (optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).

The workflow builds three platforms into one draft, one job after another: macOS on Apple Silicon (signed and notarized), Linux x86_64 (`.deb` and AppImage) and Windows x86_64 (NSIS installer). The Linux and Windows jobs install and run what they built (`scripts/smoke-linux.sh`, `scripts/smoke-windows.ps1`) and fail the release if the app does not start, draw its window, stay a single process, or come back after its window is closed.

macOS and Linux are published. The Windows installer is built and tested but withheld at promotion until it is code-signed (see [Windows code signing](#windows-code-signing)).

Do not put those secrets in `ci.yml`. That workflow runs on every pull request.

## The updater key

The updater minisign **private** key lives on Environment `release` only (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Never commit it. The matching public key is baked into the app (`plugins.updater.pubkey` in `tauri.conf.json` and the desktop-crate constant). CI fails closed if that public key is missing or empty. Do not invent a throwaway public key.

## Key ceremony

The updater keypair is generated offline; its private half never touches the repository.

### Updater minisign keypair

Tauri app updates are signed with minisign.

1. On a machine with `@tauri-apps/cli` installed (or via `npx @tauri-apps/cli signer generate`):
   ```
   npx @tauri-apps/cli signer generate
   ```
   This prompts for a password (optional) and outputs `skey.txt` (private) and `pubkey.txt` (public).

2. The private key `skey.txt` becomes the GitHub Environment `release` secret `TAURI_SIGNING_PRIVATE_KEY` (and optionally `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).

3. The public key from `pubkey.txt` is baked into two places:
   - `plugins.updater.pubkey` in `apps/desktop/src-tauri/tauri.conf.json`
   - The Rust constant in `apps/desktop/src-tauri/src/update_key.rs`

## Cut a build

1. Set the same version in `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json`, and `web/package.json`, refresh `Cargo.lock` (for example with `cargo check`), and merge the change. A test in the desktop crate fails if the three versions differ. The app compares its own version with the update feed, so the tag must match.
2. Confirm `ci.yml` is green on `main`.
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. The Release workflow runs on that tag and uses Environment `release`.
5. The workflow leaves a draft release. Download and test the build from the draft, then promote it (next section). The `latest.json` that tauri-action uploads is discarded; promotion assembles and signs the real one.

After the build, the macOS job checks the app with `codesign --verify --deep --strict`, requires a Developer ID Application authority, the team in `APPLE_TEAM_ID` and the hardened runtime, and requires `spctl` to report it notarized and `xcrun stapler validate` to pass. Tauri does not notarize the disk image, so the job notarizes it with the same App Store Connect key, staples it, checks it with `spctl` and `stapler`, and replaces the copy on the draft. Any failed check fails the release.

Until the Apple secrets are present, the macOS job **fails closed**: it will not publish an unsigned Mac build as if it were signed, and the Linux and Windows jobs, which run after it, do not start. Every job **fails closed** if `TAURI_SIGNING_PRIVATE_KEY` is empty, so no installer reaches the draft without its updater signature. macOS uses Tauri's official `APPLE_*` environment variables once those secrets are set.

Optional: Actions → Release → Run workflow with `dry_run` still requires Environment `release` and does not attach a GitHub Release. Start it from `main`: the environment accepts deployments only from `main` and `v*` tags.

## Promote the draft to a published release

A tag push builds a **draft** release in this repo. A draft is visible only to people with write access, and the updater cannot see it: the app reads `releases/latest`, which GitHub resolves to published releases only. Test the signed build from the draft, then promote it:

1. Dry run first: `gh workflow run promote.yml -f tag=vX.Y.Z -f dry_run=true`. This assembles, signs, and verifies the feed without changing the release.
2. Promote: `gh workflow run promote.yml -f tag=vX.Y.Z` (uses Environment `release`).
3. The workflow refuses anything that is not a draft, downloads the draft's artifacts, assembles and signs `latest.json` with the updater minisign key, and verifies the signature with the app's baked public key.
4. It then deletes every asset the release set does not publish, uploads the signed feed, and publishes the draft as the latest release. Publishing is the last step, so a failure leaves a draft to fix, never a half-published release.
5. The published release holds the `.dmg`, `.app.tar.gz`, `.AppImage` and `.deb` with their signatures, plus `latest.json` and its signature, the version-free copies below, and a `SHA256SUMS` file listing every one of them (`sha256sum --check SHA256SUMS`). The feed has a `darwin-aarch64` and a `linux-x86_64` entry. With `-f publish_windows=true` it also holds the `-setup.exe` and a `windows-x86_64` entry.

### Version-free download names

getoikonomia.app links to `https://github.com/ourovoros-io/oikonomia/releases/latest/download/<name>`, so every published release carries a copy of each download under a name without the version:

| Site link | File |
| --- | --- |
| `/download/macos` | `Oikonomia-macos-arm64.dmg` |
| `/download/windows` | `Oikonomia-windows-x64-setup.exe` (only with `publish_windows=true`) |
| `/download/linux-appimage` | `Oikonomia-linux-x86_64.AppImage` |
| `/download/linux-deb` | `Oikonomia-linux-amd64.deb` |
| `/download/checksums` | `SHA256SUMS` |

Promotion makes the copies after the feed is signed and verified, lists them in `SHA256SUMS`, uploads them with the feed, checks every one is on the draft, and only then publishes. A missing source file stops the promotion. The names are constants in `release_set.rs`, pinned by a test; change them only together with the site. The feed keeps naming the versioned files.

Which file fills which feed entry, and which assets survive, is decided by `crates/oikonomia-update/src/release_set.rs` and covered by its tests. Promotion stops if a platform has no artifact or more than one.

A `.deb` install is never updated by the app: it reports the newer version and the user installs the new `.deb`.

Installed apps are offered the update as soon as the draft is published. To withdraw a bad release, convert it back to a draft or delete it; apps then see the previous published release again.

### Secrets

Promotion needs only the updater key already on Environment `release` (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). It publishes with the workflow's own token, so there is no cross-repository access token to manage.

Before tagging, confirm under Settings, Environments that `release` has required reviewers and is restricted to `main` and `v*` tags. Every release and promotion then waits for an explicit approval, and the updater key is not readable from other branches.

## Windows code signing

The Windows installer has no code-signing certificate. Unsigned, a first install shows the "Windows protected your PC" warning, so promotion withholds the installer unless run with `-f publish_windows=true`.

The in-app update does not depend on that certificate: it verifies the updater minisign signature, exactly as on the other platforms.

When a certificate exists, sign in the `windows` job of `release.yml` (Tauri's `bundle.windows.signCommand`, or the provider's own action) before the smoke test, then promote with `publish_windows=true`. The install is per user (`bundle.windows.nsis.installMode`), which the update path relies on: it starts the new installer without elevation.
