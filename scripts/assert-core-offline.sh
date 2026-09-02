#!/usr/bin/env bash
# oikonomia-core must stay fully offline: the scoped deny.toml wrappers allow
# network crates under the updater, so this guards the core subtree itself.
set -euo pipefail
cd "$(dirname "$0")/.."
banned='reqwest|ureq|isahc|attohttpc|minreq|curl|curl-sys|hyper|hyper-util|h2|tungstenite|tokio-tungstenite|socket2|mio|rustls|webpki-roots|native-tls'
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  if cargo tree -p oikonomia-core -e normal --target "$target" --prefix none \
    | grep -E "^(${banned}) v"; then
    echo "error: network-capable crate inside oikonomia-core for ${target}" >&2
    exit 1
  fi
done
echo "oikonomia-core is offline on all desktop targets"
