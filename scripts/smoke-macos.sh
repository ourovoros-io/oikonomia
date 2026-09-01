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
