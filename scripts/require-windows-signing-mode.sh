#!/usr/bin/env bash
# Fail unless WINDOWS_SIGNING records a deliberate choice about how the
# Windows installer is signed.
#
# No Authenticode signing is implemented yet, so the one accepted value is
# `none`: build and publish the installer without Authenticode, on purpose.
# Unset, empty or any other value fails, so the installer is never shipped
# unsigned by accident, and a typo or a half-configured signing provider
# never drops Windows from a release without anyone noticing.
#
# This is about Authenticode only. The in-app update is verified with the
# updater minisign key on every platform, Windows included, whatever this
# says.
#
# Used by release.yml and promote.yml. Reads WINDOWS_SIGNING from the
# environment (the workflows pass the repository variable of that name).
set -euo pipefail

case "${WINDOWS_SIGNING:-}" in
  none)
    echo "::warning title=Windows installer is not Authenticode-signed::WINDOWS_SIGNING=none. Windows SmartScreen warns on a first install. In-app updates are still verified with the updater minisign key."
    ;;
  "")
    echo "::error title=WINDOWS_SIGNING is not set::Refusing to build or publish a Windows installer without a deliberate signing choice."
    echo "No Authenticode signing is set up. To ship the installer unsigned on purpose, add the"
    echo "repository variable WINDOWS_SIGNING with the value none:"
    echo "  Settings > Secrets and variables > Actions > Variables > New repository variable"
    echo "  or: gh variable set WINDOWS_SIGNING --body none --repo <owner>/<repo>"
    exit 1
    ;;
  *)
    echo "::error title=Unsupported WINDOWS_SIGNING::WINDOWS_SIGNING is set, but not to 'none', the only mode this repository implements."
    echo "Authenticode signing is not wired into release.yml yet. Set WINDOWS_SIGNING to none to"
    echo "ship the installer unsigned on purpose, or add the signing step before using another value."
    exit 1
    ;;
esac
