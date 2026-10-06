#!/usr/bin/env bash
# Smoke-test the BUNDLED app. The bare cargo binary renders a blank window
# even on healthy code, so only the .app bundle proves anything.
set -euo pipefail
cd "$(dirname "$0")/.."

app="target/release/bundle/macos/Oikonomia.app"
# The Tauri CLI pinned in web/package-lock.json, the copy the CI workflows run.
tauri_cli="web/node_modules/@tauri-apps/cli/tauri.js"
if [ "${SMOKE_SKIP_BUILD:-0}" != "1" ]; then
  [ -f "$tauri_cli" ] || {
    echo "error: $tauri_cli missing; run 'npm ci' in web/ first" >&2
    exit 1
  }
  # Local smoke needs no updater artifacts; the release lane builds them with the real key.
  overlay="$(mktemp -t oikonomia-smoke-config)"
  printf '{"bundle": {"createUpdaterArtifacts": false}}' >"$overlay"
  node "$tauri_cli" build --bundles app --config "$overlay" -- --locked
  rm -f "$overlay"
fi
[ -d "$app" ] || {
  echo "error: $app missing" >&2
  exit 1
}

open "$app"
trap 'osascript -e "tell application \"Oikonomia\" to quit" >/dev/null 2>&1 || true' EXIT
sleep 10

swift scripts/smoke-window-check.swift
echo "smoke ok: the bundled app rendered a real window"
