#!/bin/sh
# Lounge as the TV's Home: home.sh enable [APP_DIR [HOME_ID]] | disable | status.
# Lounge copies this to /var/lib/lounge and runs it as root; the work is done by Lounge's own
# executable (`lounge --home COMMAND`, src/home-mount.rs) from the installed package. The
# Homebrew Channel runs the copy at boot through /var/lib/webosbrew/init.d/10-lounge-home with
# no arguments (run-parts skips names with a dot, so the hook has none). The hook is a symlink:
# nothing here is looked up beside $0.
# Give Home back to LG by hand: sh /var/lib/lounge/home.sh disable. That needs only this file:
# where Lounge's executable is missing, fails or hangs, the script gives Home back itself, for
# every way Lounge can be Home: its executable mounted over LG Home's (com.webos.app.home, or
# com.webos.app.fullhome on some webOS 5 and 6 TVs), or its staged Home app mounted over LG
# Home's app folder (webOS 10 and 11), which sam is then made to read again.
STATE=/var/lib/lounge
HOOK=/var/lib/webosbrew/init.d/10-lounge-home
MARKER=/var/run/lounge-home
TARGET_MARKER=/var/run/lounge-home-target
STAGE=/var/lib/lounge/app
APPS=/usr/palm/applications
SCHEMA=/etc/palm/schemas/sam/applicationManager.blockSystemApp.schema
MOUNTS=/proc/self/mountinfo
PROC=/proc
LOCK=/tmp/lounge-home.lock
# The boot step's own lines, kept only with diagnostics switched on (as Lounge's other logs).
BOOT_LOG=/dev/null
[ -e /var/lib/lounge/debug ] || [ -e /tmp/lounge-debug ] && BOOT_LOG=/tmp/lounge-home-boot.log
LUNA=/usr/bin/luna-send
# Where the TV installs Lounge (tv/install-directories.txt).
DIRS='/media/developer/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge /media/cryptofs/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge'
APP=io.github.yashwanthkamireddi.lounge
COMMAND=${1:-boot}
case "$COMMAND" in
  enable|disable|status|apply|refresh|boot) ;;
  *) printf '%s\n' 'Usage: home.sh enable [APP_DIR [HOME_ID]] | disable | status' >&2; exit 1 ;;
esac
[ "$(id -u)" = 0 ] || { printf '%s\n' '{"returnValue":false,"errorText":"Run as root"}'; exit 1; }
# Lounge may stop the process that started this (itself, as Home): finish regardless.
trap '' PIPE HUP
# The package the saved settings name.
saved() { sed -n 's/.*"appDir": "\([^"]*\)".*/\1/p' "$STATE/home.json" 2>/dev/null; }
# The Home Lounge stands in for, and how (only these two Homes and two ways are Lounge's).
HOME_ID=$(sed -n 's/.*"homeId": "\([^"]*\)".*/\1/p' "$STATE/home.json" 2>/dev/null)
case "$HOME_ID" in com.webos.app.fullhome) ;; *) HOME_ID=com.webos.app.home ;; esac
METHOD=$(sed -n 's/.*"method": "\([^"]*\)".*/\1/p' "$STATE/home.json" 2>/dev/null)
case "$METHOD" in manifest) ;; *) METHOD=executable ;; esac
EXE=/usr/bin/$HOME_ID
# The root of the mount on top of $1 (LG Home's executable by default), if any.
mount_root() { awk -v exe="${1:-$EXE}" '$5 == exe { root = $4 } END { print root }' "$MOUNTS"; }
# Lounge's executable, also after a package update replaced the mounted file.
ours() {
  case "$1" in */$APP/lounge|*/$APP/lounge//deleted) return 0 ;; esac
  return 1
}
# Lounge's staged Home folder (/lib/lounge/app where /var is its own file system).
staged() {
  case "$1" in */lounge/app|*/lounge/app//deleted) return 0 ;; esac
  return 1
}
# Anything but Lounge's staged Home mounted over, or inside, LG Home's app folder $1.
manifest_mounted() {
  awk -v dir="$APPS/$1" '$5 == dir && $4 !~ "/lounge/app(//deleted)?$" { found = 1 }
    index($5, dir "/") == 1 { found = 1 } END { exit !found }' "$MOUNTS"
}
if [ "$COMMAND" = boot ]; then
  # Nothing at all happens at boot unless Lounge is enabled as Home and still installed.
  grep -q '"enabled": true' "$STATE/home.json" 2>/dev/null || exit 0
  APP_DIR=$(saved)
  [ -n "$APP_DIR" ] && [ -x "$APP_DIR/lounge" ] || exit 0
  # Other boot hooks must not wait for this one.
  (
    exec 9>"$LOCK"
    flock -w 120 9 || exit 1
    exec "$APP_DIR/lounge" --home boot
  ) </dev/null >>"$BOOT_LOG" 2>&1 &
  exit 0
fi
if [ "$COMMAND" = refresh ]; then
  # Lounge as Home found its mount holding an executable a package update replaced. Home is
  # waiting on it: plain sh, mounting the new file only; nothing is stopped.
  exec 9>"$LOCK"
  flock -w 30 9 || exit 1
  grep -q '"enabled": true' "$STATE/home.json" 2>/dev/null || exit 1
  APP_DIR=$(saved)
  [ -n "$APP_DIR" ] && [ -x "$APP_DIR/lounge" ] || exit 1
  ours "$(mount_root)" || { printf '%s\n' '{"returnValue":false,"errorText":"Lounge is not mounted as Home"}'; exit 1; }
  [ "$METHOD" = executable ] || exit 1
  manifest_mounted "$HOME_ID" && exit 1
  if [ "$(stat -L -c %d:%i "$APP_DIR/lounge")" != "$(stat -L -c %d:%i "$EXE")" ]; then
    umount "$EXE" 2>/dev/null || umount -l "$EXE" || exit 1
    mount --bind "$APP_DIR/lounge" "$EXE" || exit 1
  fi
  [ "$(stat -L -c %d:%i "$APP_DIR/lounge")" = "$(stat -L -c %d:%i "$EXE")" ] || exit 1
  printf '%s\n' '{"returnValue":true,"mounted":true}'
  exit 0
fi
mkdir -p "$STATE" || exit 1
exec 9>"$LOCK"
flock -w 120 9 || { printf '%s\n' '{"returnValue":false,"errorText":"Lounge Home is already being changed"}'; exit 1; }
HELPER=
for DIR in ${2:+"$2"} $(saved) $DIRS; do
  [ -x "$DIR/lounge" ] && { HELPER=$DIR/lounge; break; }
done
if [ "$COMMAND" != disable ]; then
  [ -n "$HELPER" ] || { printf '%s\n' '{"returnValue":false,"errorText":"No installed native Lounge package found"}'; exit 1; }
  exec "$HELPER" --home "$COMMAND" "$2" ${3:+"$3"}
fi
# disable: Lounge's executable first, bounded; anything but a successful reply and this
# script restores LG's Home on its own.
REPLY=
if [ -n "$HELPER" ]; then
  if [ -x /usr/bin/timeout ]; then
    REPLY=$(/usr/bin/timeout -s KILL 60 "$HELPER" --home disable)
  else
    REPLY=$("$HELPER" --home disable)
  fi
fi
case "$REPLY" in
  *'"returnValue":true'*) printf '%s\n' "$REPLY"; exit 0 ;;
esac
printf 'Lounge Home: %s; restoring LG Home from the script\n' "${REPLY:-Lounge did not answer}" >&2
fail() { printf '{"returnValue":false,"errorText":"%s"}\n' "$1"; exit 1; }
alive() {
  for P; do kill -0 "$P" 2>/dev/null && return 0; done
  return 1
}
FRONT=$("$LUNA" -n 1 -w 5000 luna://com.webos.applicationManager/getForegroundAppInfo '{}' 2>/dev/null | tr -d '\n')
case "$FRONT" in
  *'"returnValue": true'*|*'"returnValue":true'*)
    FRONT=$(printf '%s' "$FRONT" | sed -n 's/.*"appId": *"\([^"]*\)".*/\1/p')
    KNOWN=1 ;;
  *) KNOWN= ;;
esac
if [ -f "$STATE/home.json" ]; then
  sed 's/"enabled": true/"enabled": false/' "$STATE/home.json" >"$STATE/home.json.new" &&
    mv "$STATE/home.json.new" "$STATE/home.json"
fi
ROOT=$(mount_root)
[ -z "$ROOT" ] || ours "$ROOT" || fail 'Another integration is mounted over LG Home'
rm -f "$MARKER" "$TARGET_MARKER"
[ "$(readlink "$HOOK")" = "$STATE/home.sh" ] && rm -f "$HOOK"
# Lounge's mount over either Home; another integration's over the other one stays.
for ID in com.webos.app.home com.webos.app.fullhome; do
  ours "$(mount_root "/usr/bin/$ID")" || continue
  i=0
  while [ $i -lt 3 ] && [ -n "$(mount_root "/usr/bin/$ID")" ]; do
    ours "$(mount_root "/usr/bin/$ID")" || fail 'Another integration is mounted over LG Home'
    # A running Home executes the mounted file: detach it instead.
    umount "/usr/bin/$ID" 2>/dev/null || umount -l "/usr/bin/$ID"
    i=$((i + 1))
  done
  [ -z "$(mount_root "/usr/bin/$ID")" ] || fail 'The Lounge Home mount is still active'
done
# The staged Home (webOS 10 and 11): off LG Home's app folder, then sam reads LG's manifest
# again. A block left in place would keep Home from starting: the release always follows.
STAGED=
i=0
while [ $i -lt 3 ] && staged "$(mount_root "$APPS/com.webos.app.home")"; do
  STAGED=1
  umount "$APPS/com.webos.app.home" 2>/dev/null || umount -l "$APPS/com.webos.app.home"
  i=$((i + 1))
done
staged "$(mount_root "$APPS/com.webos.app.home")" && fail 'The Lounge Home mount is still active'
sam_reads_lounge() {
  INFO=$("$LUNA" -n 1 -w 5000 luna://com.webos.applicationManager/getAppInfo '{"id":"com.webos.app.home"}' 2>/dev/null | tr -d '\n')
  case "$INFO" in
    *'"returnValue": true'*|*'"returnValue":true'*) ;;
    *) return 2 ;;
  esac
  printf '%s' "$INFO" | grep -Eq '"main": *"([^"]*/)?lounge"'
}
if [ -n "$STAGED" ] || [ "$METHOD" = manifest ]; then
  sam_reads_lounge
  if [ $? != 1 ]; then
    BLOCK='{"id":"com.webos.app.home"}'
    grep -q '"forceBlockSystemApp"' "$SCHEMA" 2>/dev/null &&
      BLOCK='{"id":"com.webos.app.home","forceBlockSystemApp":false}'
    "$LUNA" -n 1 -w 5000 luna://com.webos.applicationManager/blockSystemApp "$BLOCK" >/dev/null 2>&1
    i=0
    while [ $i -lt 3 ]; do
      RELEASED=$("$LUNA" -n 1 -w 5000 luna://com.webos.applicationManager/releaseBlockSystemApp '{"id":"com.webos.app.home"}' 2>/dev/null | tr -d '\n')
      case "$RELEASED" in
        *'"returnValue": true'*|*'"returnValue":true'*|*[Nn]ot*[Bb]locked*) break ;;
      esac
      sleep 1
      i=$((i + 1))
    done
    [ $i -lt 3 ] || fail 'Release Home failed'
    i=0
    while [ $i -lt 15 ]; do
      sam_reads_lounge
      [ $? = 1 ] && break
      sleep 1
      i=$((i + 1))
    done
    [ $i -lt 15 ] || fail "sam did not read Home's manifest again"
  fi
fi
if [ -d "$STAGE" ] && ! staged "$(mount_root "$APPS/com.webos.app.home")"; then
  # Links are removed, never followed: the package stays.
  rm -f "$STAGE"/* 2>/dev/null
  rmdir "$STAGE" 2>/dev/null
fi
# Every process started as that Home: LG's executable, LG's Flutter Home, or Lounge started
# from the staged Home. sam starts LG's own in its place.
PIDS=
for P in "$PROC"/[0-9]*; do
  ARGS=$(tr '\0' '\n' <"$P/cmdline" 2>/dev/null)
  ARGV0=$(printf '%s\n' "$ARGS" | head -n 1)
  case "$ARGV0" in
    "$EXE") ;;
    */flutter-client|flutter-client)
      printf '%s\n' "$ARGS" | awk -v id="$HOME_ID" 'prev == "-i" && $0 == id { found = 1 } { prev = $0 } END { exit !found }' || continue ;;
    */lounge) printf '%s\n' "$ARGS" | sed -n 2p | grep -Eq "\"(nid|appId)\": *\"$HOME_ID\"" || continue ;;
    *) continue ;;
  esac
  PIDS="$PIDS ${P##*/}"
done
if [ -n "$PIDS" ]; then
  kill -TERM $PIDS 2>/dev/null
  i=0
  while [ $i -lt 8 ] && alive $PIDS; do sleep 1; i=$((i + 1)); done
  alive $PIDS && kill -KILL $PIDS 2>/dev/null
  i=0
  while [ $i -lt 3 ] && alive $PIDS; do sleep 1; i=$((i + 1)); done
  alive $PIDS && fail 'The previous Home process has not stopped'
fi
# Nothing in front means Home itself went away: LG's Home takes its place.
if [ -n "$KNOWN" ] && { [ "$FRONT" = "$HOME_ID" ] || [ -z "$FRONT" ]; }; then
  "$LUNA" -n 1 -w 5000 luna://com.webos.applicationManager/launch "{\"id\":\"$HOME_ID\"}" >/dev/null 2>&1
fi
CHECKPOINT=false
if [ -e /tmp/checkpoint/$HOME_ID ] || [ -n "$(ls -A /mnt/lg/cmn_data/checkpoint/$HOME_ID 2>/dev/null)" ]; then
  CHECKPOINT=true
fi
FOREIGN=false
manifest_mounted "$HOME_ID" && FOREIGN=true
APP_DIR=$(saved)
if [ -n "$APP_DIR" ]; then APP_DIR="\"$APP_DIR\""; else APP_DIR=null; fi
TARGET=
[ "$HOME_ID" = com.webos.app.home ] || TARGET=",\"homeId\":\"$HOME_ID\""
printf '{"returnValue":true,"method":"%s","enabled":false,"mounted":false,"stale":false,"foreign":%s,"hook":false,"marker":false,"checkpoint":%s,"appDir":%s%s}\n' \
  "$METHOD" "$FOREIGN" "$CHECKPOINT" "$APP_DIR" "$TARGET"
