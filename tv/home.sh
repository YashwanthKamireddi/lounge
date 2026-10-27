#!/bin/sh
# Lounge as the TV's Home: home.sh enable [APP_DIR] | disable | status.
# Lounge copies this and home.cjs to /var/lib/lounge and runs them as root. The Homebrew
# Channel runs the copy at boot through /var/lib/webosbrew/init.d/10-lounge-home with no
# arguments (run-parts skips names with a dot, so the hook has none).
# Give Home back to LG by hand: sh /var/lib/lounge/home.sh disable
STATE=/var/lib/lounge
COMMAND=${1:-boot}
case "$COMMAND" in
  enable|disable|status|apply|refresh|boot) ;;
  *) printf '%s\n' 'Usage: home.sh enable [APP_DIR] | disable | status' >&2; exit 1 ;;
esac
[ "$(id -u)" = 0 ] || { printf '%s\n' '{"returnValue":false,"errorText":"Run as root"}'; exit 1; }
# Lounge may stop the process that started this (itself, as Home): finish regardless.
trap '' PIPE HUP
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
if [ "$COMMAND" = boot ]; then
  # Nothing at all happens at boot unless Lounge is enabled as Home and still installed.
  grep -q '"enabled": true' "$STATE/home.json" 2>/dev/null || exit 0
  APP_DIR=$(sed -n 's/.*"appDir": "\([^"]*\)".*/\1/p' "$STATE/home.json")
  [ -n "$APP_DIR" ] && [ -x "$APP_DIR/lounge" ] || exit 0
  [ -x /usr/bin/node ] || exit 0
  # Other boot hooks must not wait for this one.
  (
    exec 9>/tmp/lounge-home.lock
    flock -w 120 9 || exit 1
    exec /usr/bin/node "$SCRIPT_DIR/home.cjs" boot
  ) </dev/null >>/tmp/lounge-home-boot.log 2>&1 &
  exit 0
fi
if [ "$COMMAND" = refresh ]; then
  # Lounge as Home found its mount holding an executable a package update replaced. Plain sh,
  # not Node: Home is waiting on it. Mounts the new file only; nothing is stopped.
  EXE=/usr/bin/com.webos.app.home
  exec 9>/tmp/lounge-home.lock
  flock -w 30 9 || exit 1
  grep -q '"enabled": true' "$STATE/home.json" 2>/dev/null || exit 1
  APP_DIR=$(sed -n 's/.*"appDir": "\([^"]*\)".*/\1/p' "$STATE/home.json")
  [ -n "$APP_DIR" ] && [ -x "$APP_DIR/lounge" ] || exit 1
  ROOT=$(awk -v exe="$EXE" '$5 == exe { root = $4 } END { print root }' /proc/self/mountinfo)
  case "$ROOT" in
    */io.github.yashwanthkamireddi.lounge/lounge|*/io.github.yashwanthkamireddi.lounge/lounge//deleted) ;;
    *) printf '%s\n' '{"returnValue":false,"errorText":"Lounge is not mounted as Home"}'; exit 1 ;;
  esac
  awk '$5 ~ "^/usr/palm/applications/com.webos.app.home(/|$)" { found = 1 } END { exit !found }' /proc/self/mountinfo && exit 1
  if [ "$(stat -L -c %d:%i "$APP_DIR/lounge")" != "$(stat -L -c %d:%i "$EXE")" ]; then
    umount "$EXE" 2>/dev/null || umount -l "$EXE" || exit 1
    mount --bind "$APP_DIR/lounge" "$EXE" || exit 1
  fi
  [ "$(stat -L -c %d:%i "$APP_DIR/lounge")" = "$(stat -L -c %d:%i "$EXE")" ] || exit 1
  printf '%s\n' '{"returnValue":true,"mounted":true}'
  exit 0
fi
[ -x /usr/bin/node ] || { printf '%s\n' '{"returnValue":false,"errorText":"The TV has no Node.js runtime"}'; exit 1; }
mkdir -p "$STATE" || exit 1
exec 9>/tmp/lounge-home.lock
flock -w 120 9 || { printf '%s\n' '{"returnValue":false,"errorText":"Lounge Home is already being changed"}'; exit 1; }
exec /usr/bin/node "$SCRIPT_DIR/home.cjs" "$COMMAND" "$2"
