#!/usr/bin/env bash
# Smoke-test the BUNDLED Linux app: the .deb, then the AppImage.
#
# For each one it proves, on a desktop with no system tray (the stock GNOME
# case):
#   - the app starts and draws a real window;
#   - typing a password creates the encrypted vault, owner-only;
#   - a second launch exits and leaves one process (single instance);
#   - closing the window hides it and keeps the app alive;
#   - launching again brings the hidden window back;
#   - the vault directory is created owner-only under XDG_DATA_HOME.
#
# Needs: xvfb, openbox, xdotool, wmctrl, imagemagick, dbus-x11.
# Screenshots land in target/smoke/ for a human to look at.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${SMOKE_INNER:-}" ]; then
  # Re-run inside a private D-Bus session (the single-instance check uses the
  # session bus) and a virtual display.
  exec env SMOKE_INNER=1 dbus-run-session -- \
    xvfb-run --auto-servernum --server-args="-screen 0 1440x900x24" "$0" "$@"
fi

readonly WINDOW_TITLE='^Oikonomia$'
readonly PROCESS_NAME='oikonomia'
readonly WAIT_SECONDS=60
# How long a second launch may take to hand over and exit. Generous because
# the AppImage unpacks itself on every launch here.
readonly LAUNCH_TIMEOUT=120
# A blank webview is one flat colour; the real unlock screen has thousands.
readonly MIN_DISTINCT_COLOURS=64
readonly PASSWORD='correct horse battery staple'

out="target/smoke"
mkdir -p "$out"

fail() {
  echo "smoke FAILED: $*" >&2
  exit 1
}

visible_window() {
  xdotool search --onlyvisible --name "$WINDOW_TITLE" 2>/dev/null | head -n 1 || true
}

process_count() {
  pgrep -c -x "$PROCESS_NAME" || true
}

# wait_until <description> <command...>: poll until the command succeeds.
wait_until() {
  local description="$1"
  shift
  local waited=0
  until "$@"; do
    waited=$((waited + 1))
    [ "$waited" -lt "$WAIT_SECONDS" ] || fail "timed out waiting until $description"
    sleep 1
  done
}

window_is_visible() { [ -n "$(visible_window)" ]; }
window_is_hidden() { [ -z "$(visible_window)" ]; }
app_has_exited() { [ "$(process_count)" -eq 0 ]; }

stop_app() {
  pkill -x "$PROCESS_NAME" 2>/dev/null || true
  wait_until "the app has exited" app_has_exited
}

# smoke <name> <launch command...>
smoke() {
  local name="$1"
  shift
  echo "== $name"

  # A private data directory per bundle, so each run starts with no vault.
  export XDG_DATA_HOME="$PWD/$out/$name-data"
  rm -rf "$XDG_DATA_HOME"

  "$@" >"$out/$name.log" 2>&1 &
  wait_until "$name shows its window" window_is_visible

  # Give the webview time to load and paint the first screen.
  sleep 8
  local window
  window="$(visible_window)"
  import -window "$window" "$out/$name.png"
  local colours
  colours="$(identify -format '%k' "$out/$name.png")"
  echo "$name: window drawn with $colours distinct colours"
  [ "$colours" -ge "$MIN_DISTINCT_COLOURS" ] \
    || fail "$name drew a blank window ($colours colours); see $out/$name.png"

  local vault_dir="$XDG_DATA_HOME/oikonomia"
  [ -d "$vault_dir" ] || fail "$name did not create $vault_dir"
  local mode
  mode="$(stat -c '%a' "$vault_dir")"
  [ "$mode" = "700" ] || fail "$name vault directory mode is $mode, expected 700"

  # Create a vault through the real first-run screen: the password field has
  # focus, Tab moves to the confirmation, Enter submits.
  xdotool windowactivate --sync "$window"
  xdotool type --delay 40 "$PASSWORD"
  xdotool key Tab
  xdotool type --delay 40 "$PASSWORD"
  xdotool key Return
  vault_is_created() { [ -f "$vault_dir/vault.db" ] && [ -f "$vault_dir/vault.header.json" ]; }
  wait_until "$name creates the encrypted vault" vault_is_created
  local file
  for file in vault.db vault.header.json; do
    mode="$(stat -c '%a' "$vault_dir/$file")"
    [ "$mode" = "600" ] || fail "$name $file mode is $mode, expected 600"
  done
  # SQLCipher leaves no readable SQLite header; a plaintext database starts with it.
  if head -c 15 "$vault_dir/vault.db" | grep -q 'SQLite format 3'; then
    fail "$name wrote a plaintext database"
  fi
  sleep 5
  import -window "$window" "$out/$name-unlocked.png"

  timeout "$LAUNCH_TIMEOUT" "$@" >"$out/$name-second.log" 2>&1 \
    || fail "$name: a second launch did not exit cleanly"
  [ "$(process_count)" -eq 1 ] \
    || fail "$name: expected one process after a second launch, found $(process_count)"

  wmctrl -c 'Oikonomia'
  wait_until "$name hides its window on close" window_is_hidden
  [ "$(process_count)" -eq 1 ] || fail "$name quit when its window was closed"

  timeout "$LAUNCH_TIMEOUT" "$@" >"$out/$name-reopen.log" 2>&1 \
    || fail "$name: the reopening launch did not exit cleanly"
  wait_until "$name shows its hidden window again" window_is_visible
  [ "$(process_count)" -eq 1 ] \
    || fail "$name: expected one process after reopening, found $(process_count)"

  stop_app
  echo "$name ok"
}

shopt -s nullglob
debs=(target/release/bundle/deb/*.deb)
appimages=(target/release/bundle/appimage/*.AppImage)
[ "${#debs[@]}" -eq 1 ] || fail "expected one .deb, found ${#debs[@]}"
[ "${#appimages[@]}" -eq 1 ] || fail "expected one AppImage, found ${#appimages[@]}"

# A window manager is what turns "close" into a close request, as on a real desktop.
openbox &
sleep 2

# Installing with apt proves the package's declared dependencies resolve.
sudo apt-get install -y "./${debs[0]}"
smoke deb "$PROCESS_NAME"
sudo apt-get remove -y "$PROCESS_NAME"

# CI runners have no FUSE; the AppImage runtime can unpack itself instead.
chmod +x "${appimages[0]}"
export APPIMAGE_EXTRACT_AND_RUN=1
smoke appimage "$PWD/${appimages[0]}"

echo "smoke ok: the .deb and the AppImage start, draw, stay single, and come back from hidden"
