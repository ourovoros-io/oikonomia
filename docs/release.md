# Cutting a desktop release

Ops note for the tag-gated desktop workflow. Not a product README.

## GitHub Environment `release`

Workflow YAML cannot create Environments.

1. GitHub → Settings → Environments → New environment.
2. Name it exactly `release`.
3. Add required reviewers.
4. After Apple Developer (individual team, no App Store) and Windows cloud-HSM (SSL.com eSigner or DigiCert KeyLocker) accounts exist, add **only** the secret *names* listed at the top of [`.github/workflows/release.yml`](../.github/workflows/release.yml). Include `TAURI_SIGNING_PRIVATE_KEY` (optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). That is the updater minisign key, not the license key.

Do not put those secrets in `ci.yml`. That workflow runs on every pull request.

## What never lives on GitHub

- License-signing **private** key (mint on a host you control only)
- Paddle API keys / webhook secrets
- Any buyer PII
- A raw Authenticode `.pfx` of the Windows code-signing private key

The updater minisign **private** key lives on Environment `release` only (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). It is not the license signing key. Never commit it. The matching public key is baked into the app (`plugins.updater.pubkey` in `tauri.conf.json` and the desktop-crate constant). That key is not the license Ed25519 key. CI fails closed if that public key is missing or empty. Do not invent a throwaway public key.

## Cut a build

1. Confirm `ci.yml` is green on `main`.
2. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The Release workflow runs on that tag and uses Environment `release`.
4. Download installers from the GitHub Release for that tag. GitHub Releases is the v1 updater CDN (`latest.json`). Trust is the updater minisign public key baked into the app. Linux in-app updates are AppImage only; `.deb` remains a manual download. Unlock-screen `update_check` / `update_install` live in the desktop crate and exec the wrapper-verified local path; HTTP is not on `oikonomia-core` or `license.rs`. Ops owns `uploadUpdaterJson` / `uploadUpdaterSignatures` in the release workflow.

Until Apple and Windows HSM secrets are present, the macOS and Windows jobs **fail closed**. They will not publish an unsigned Mac/Windows build as if it were signed. Linux stays unsigned for code signing (`.deb` / `.AppImage`). All three jobs **fail closed** if `TAURI_SIGNING_PRIVATE_KEY` is empty — they will not publish updater JSON (`latest.json`) without it.

Windows Authenticode is still a placeholder even after HSM secrets exist: replace `.github/scripts/windows-cloud-hsm-sign.ps1` with a live CodeSignTool or `smctl` invocation at go-live. macOS uses Tauri’s official `APPLE_*` environment variables once those secrets are set.

Optional: Actions → Release → Run workflow with `dry_run` still requires Environment `release` and does not attach a GitHub Release.
