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

Only macOS on Apple Silicon is published. The workflow also builds unsigned Linux assets (`.deb` and AppImage) into the draft to prove the build; promotion removes them. Windows is not built by this workflow.

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

Until the Apple secrets are present, the macOS job **fails closed**: it will not publish an unsigned Mac build as if it were signed. The Linux job builds unsigned `.deb` and AppImage files and is only there to prove the build. Both jobs **fail closed** if `TAURI_SIGNING_PRIVATE_KEY` is empty, so updater JSON (`latest.json`) is never produced without it. macOS uses Tauri's official `APPLE_*` environment variables once those secrets are set.

Optional: Actions → Release → Run workflow with `dry_run` still requires Environment `release` and does not attach a GitHub Release. Start it from `main`: the environment accepts deployments only from `main` and `v*` tags.

## Promote the draft to a published release

A tag push builds a **draft** release in this repo. A draft is visible only to people with write access, and the updater cannot see it: the app reads `releases/latest`, which GitHub resolves to published releases only. Test the signed build from the draft, then promote it:

1. Dry run first: `gh workflow run promote.yml -f tag=vX.Y.Z -f dry_run=true`. This assembles, signs, and verifies the feed without changing the release.
2. Promote: `gh workflow run promote.yml -f tag=vX.Y.Z` (uses Environment `release`).
3. The workflow refuses anything that is not a draft, downloads the draft's artifacts, assembles and signs `latest.json` with the updater minisign key, and verifies the signature with the app's baked public key.
4. It then deletes every asset outside the tested allow-list (`.app.tar.gz`, its `.sig`, `.dmg`, `latest.json`, `latest.json.sig`), uploads the signed feed, and publishes the draft as the latest release. Publishing is the last step, so a failure leaves a draft to fix, never a half-published release.
5. The published feed carries a single `darwin-aarch64` entry for now (macOS only; Windows and Linux updates are not supported).

Installed apps are offered the update as soon as the draft is published. To withdraw a bad release, convert it back to a draft or delete it; apps then see the previous published release again.

### Secrets

Promotion needs only the updater key already on Environment `release` (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). It publishes with the workflow's own token, so there is no cross-repository access token to manage.

Before tagging, confirm under Settings, Environments that `release` has required reviewers and is restricted to `main` and `v*` tags. Every release and promotion then waits for an explicit approval, and the updater key is not readable from other branches.
