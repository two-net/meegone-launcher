#!/usr/bin/env bash
# Runs the debug build's accessibility tour (ADR 0038, ADR 0039) on the device adb is connected
# to: every screen, in the Light and the Dark theme, at the normal font size, at 200 %, and with
# animations removed, as Accessibility's "Remove animations" does, so every screen must still be
# reached and pass (AC-18). Prints what fails and exits non-zero if anything does. See
# RELEASING.md.
#
#   tools/a11y-audit.sh               the screens of an installed, set-up MeeGone
#   tools/a11y-audit.sh --first-run   clears MeeGone's data first, so setup's screens are
#                                     checked too; for a test device, as it loses the layout
#                                     and settings
#
# Install the debug build first: (cd android && ./gradlew installDebug). The script puts the
# device's font size and animation scales back as it found them; the tour puts back the theme
# and Recent's density. It ends out of touch mode, as a keyboard leaves it, until the next touch.
set -euo pipefail

PACKAGE=io.github.twonet.meegone
EXTRA=io.github.twonet.meegone.debug.A11Y_AUDIT
TAG=MeeGoneA11y
# Each pass: a font scale, and whether animations are removed.
PASSES=("1.0 on" "2.0 on" "1.0 off")
ANIMATIONS=(animator_duration_scale transition_animation_scale window_animation_scale)
# The longest one tour may take, in seconds.
PATIENCE=900

first_run=false
for arg in "$@"; do
  case "$arg" in
    --first-run) first_run=true ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 64 ;;
  esac
done

adb get-state >/dev/null
if ! adb shell pm list packages "$PACKAGE" 2>/dev/null | grep -qx "package:$PACKAGE"; then
  echo "MeeGone isn't installed. Install the debug build: (cd android && ./gradlew installDebug)" >&2
  exit 2
fi
if ! adb shell dumpsys package "$PACKAGE" 2>/dev/null | grep -q "DEBUGGABLE"; then
  echo "The installed MeeGone is a release build, which has no tour (SEC-06)." >&2
  exit 2
fi

if $first_run; then
  model=$(adb shell getprop ro.product.model | tr -d '\r')
  read -r -p "--first-run clears MeeGone's layout and settings on $model. Continue? [y/N] " answer
  [[ "$answer" == [yY]* ]] || exit 1
  adb shell pm clear "$PACKAGE" >/dev/null
fi

# Puts back setting $2 in namespace $1 as $3 says: deleted if it had no value.
put_back() {
  if [[ "$3" == "null" || -z "$3" ]]; then
    adb shell settings delete "$1" "$2" >/dev/null
  else
    adb shell settings put "$1" "$2" "$3"
  fi
}
found=$(adb shell settings get system font_scale | tr -d '\r')
declare -a animations_found
for i in "${!ANIMATIONS[@]}"; do
  animations_found[$i]=$(adb shell settings get global "${ANIMATIONS[$i]}" | tr -d '\r')
done
restore() {
  put_back system font_scale "$found"
  for i in "${!ANIMATIONS[@]}"; do
    put_back global "${ANIMATIONS[$i]}" "${animations_found[$i]}"
  done
}
trap restore EXIT

failures=0
for pass in "${PASSES[@]}"; do
  read -r scale animations <<<"$pass"
  if [[ "$animations" == off ]]; then
    echo "== Font scale $scale, animations removed"
    for setting in "${ANIMATIONS[@]}"; do
      adb shell settings put global "$setting" 0
    done
  else
    echo "== Font scale $scale"
    restore
  fi
  adb shell settings put system font_scale "$scale"
  # The activity is made again for the new size.
  sleep 3
  adb logcat -c
  adb shell am start -n "$PACKAGE/.HomeActivity" --ez "$EXTRA" true >/dev/null
  deadline=$((SECONDS + PATIENCE))
  until adb logcat -d -v raw -s "$TAG:*" | grep -q '^DONE '; do
    if ((SECONDS > deadline)); then
      echo "The tour didn't finish within ${PATIENCE}s." >&2
      adb logcat -d -v raw -s "$TAG:*" >&2
      exit 2
    fi
    sleep 2
  done
  log=$(adb logcat -d -v raw -s "$TAG:*" | grep -E '^(START|FAIL|SKIP|DONE) ')
  echo "$log"
  failures=$((failures + $(grep -c '^FAIL ' <<<"$log" || true)))
done

if ((failures > 0)); then
  echo "$failures accessibility checks failed." >&2
  exit 1
fi
echo "Every accessibility check passed."
