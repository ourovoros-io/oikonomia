#!/usr/bin/env bash
# Fails when a release binary can be pointed at another update feed.
#
# The OIKONOMIA_UPDATE_FEED override (crates/oikonomia-update/src/debug_feed.rs)
# exists only with the `debug-feed` cargo feature, which release.yml never
# enables and which does not compile in an optimised build. This is the check
# that a build mistake did not ship it anyway: the code that reads the
# variable carries its name, so a binary without the name cannot read it.
#
# Usage: scripts/assert-no-debug-feed.sh <binary>...
# Fails closed: a binary that is missing or unreadable fails the check.
set -euo pipefail

needle='OIKONOMIA_UPDATE_FEED'

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <binary>..." >&2
  exit 2
fi

status=0
for binary in "$@"; do
  if [ ! -f "$binary" ] || [ ! -r "$binary" ]; then
    echo "::error::no readable release binary at $binary" >&2
    status=1
    continue
  fi
  if LC_ALL=C grep -a -q -F "$needle" "$binary"; then
    echo "::error::$binary contains $needle: the debug feed override was built into a release" >&2
    status=1
  else
    echo "ok: $binary does not contain $needle"
  fi
done

exit "$status"
