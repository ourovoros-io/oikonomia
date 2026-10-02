# Cutting a desktop release

Ops note for the tag-gated desktop workflow. Not a product README.

## GitHub Environment `release`

Workflow YAML cannot create Environments.

1. GitHub → Settings → Environments → New environment.
2. Name it exactly `release`.
3. Add required reviewers.
4. After Apple Developer (individual team, no App Store) and Windows cloud-HSM (SSL.com eSigner or DigiCert KeyLocker) accounts exist, add **only** the secret *names* listed at the top of [`.github/workflows/release.yml`](../.github/workflows/release.yml). Include `TAURI_SIGNING_PRIVATE_KEY` (optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).

Do not put those secrets in `ci.yml`. That workflow runs on every pull request.

## What never lives on GitHub

- A raw Authenticode `.pfx` of the Windows code-signing private key

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

1. Confirm `ci.yml` is green on `main`.
2. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The Release workflow runs on that tag and uses Environment `release`.
4. Download installers from the GitHub Release for that tag. GitHub Releases is the v1 updater CDN (`latest.json`). Trust is the updater minisign public key baked into the app. Linux in-app updates are AppImage only; `.deb` remains a manual download. Unlock-screen `update_check` / `update_install` live in the desktop crate and exec the wrapper-verified local path; HTTP is not on `oikonomia-core`. Ops owns `uploadUpdaterJson` / `uploadUpdaterSignatures` in the release workflow.

Until Apple and Windows HSM secrets are present, the macOS and Windows jobs **fail closed**. They will not publish an unsigned Mac/Windows build as if it were signed. Linux stays unsigned for code signing (`.deb` / `.AppImage`). All three jobs **fail closed** if `TAURI_SIGNING_PRIVATE_KEY` is empty — they will not publish updater JSON (`latest.json`) without it.

Windows Authenticode is still a placeholder even after HSM secrets exist: replace `.github/scripts/windows-cloud-hsm-sign.ps1` with a live CodeSignTool or `smctl` invocation at go-live. macOS uses Tauri’s official `APPLE_*` environment variables once those secrets are set.

Optional: Actions → Release → Run workflow with `dry_run` still requires Environment `release` and does not attach a GitHub Release.

## Promote the draft to a published release

A tag push builds a **draft** release in this repo. A draft is visible only to people with write access, and the updater cannot see it: the app reads `releases/latest`, which GitHub resolves to published releases only. Test the signed build from the draft, then promote it:

1. Dry run first: `gh workflow run promote.yml -f tag=vX.Y.Z -f dry_run=true`. This assembles, signs, and verifies the feed without changing the release.
2. Promote: `gh workflow run promote.yml -f tag=vX.Y.Z` (uses Environment `release`).
3. The workflow refuses anything that is not a draft, downloads the draft's artifacts, assembles and signs `latest.json` with the updater minisign key, and verifies the signature with the app's baked public key.
4. It then deletes every asset outside the tested allow-list (`.app.tar.gz`, its `.sig`, `.dmg`, `latest.json`, `latest.json.sig`), uploads the signed feed, and publishes the draft as the latest release. Publishing is the last step, so a failure leaves a draft to fix, never a half-published release.
5. The published feed carries a single `darwin-aarch64` entry in v1 (macOS only; Windows and Linux updates are not yet supported).

Installed apps are offered the update as soon as the draft is published. To withdraw a bad release, convert it back to a draft or delete it; apps then see the previous published release again.

### Secrets

Promotion needs only the updater key already on Environment `release` (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). It publishes with the workflow's own token, so there is no cross-repository access token to manage.

Once this repository is public, add required reviewers to Environment `release` (Settings, Environments): every release and promotion then waits for an explicit approval.
