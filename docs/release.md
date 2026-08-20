# Cutting a desktop release

Ops note for the tag-gated desktop workflow. Not a product README.

## GitHub Environment `release`

Workflow YAML cannot create Environments.

1. GitHub → Settings → Environments → New environment.
2. Name it exactly `release`.
3. Add required reviewers.
4. After Apple Developer (individual team, no App Store) and Windows cloud-HSM (SSL.com eSigner or DigiCert KeyLocker) accounts exist, add **only** the secret *names* listed at the top of [`.github/workflows/release.yml`](../.github/workflows/release.yml).

Do not put those secrets in `ci.yml`. That workflow runs on every pull request.

## What never lives on GitHub

- License-signing **private** key (mint on a host you control only)
- Paddle API keys / webhook secrets
- Any buyer PII
- A raw Authenticode `.pfx` of the Windows code-signing private key
- Tauri updater signing keys (there is no in-app updater)

## Cut a build

1. Confirm `ci.yml` is green on `main`.
2. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The Release workflow runs on that tag and uses Environment `release`.
4. Download installers from the GitHub Release for that tag. Day-one updates are a manual download — no in-app updater and no network phone-home.

Until Apple and Windows HSM secrets are present, the macOS and Windows jobs **fail closed**. They will not publish an unsigned Mac/Windows build as if it were signed. Linux stays unsigned (`.deb` / `.AppImage`).

Windows Authenticode is still a placeholder even after HSM secrets exist: replace `.github/scripts/windows-cloud-hsm-sign.ps1` with a live CodeSignTool or `smctl` invocation at go-live. macOS uses Tauri’s official `APPLE_*` environment variables once those secrets are set.

Optional: Actions → Release → Run workflow with `dry_run` still requires Environment `release` and does not attach a GitHub Release.
