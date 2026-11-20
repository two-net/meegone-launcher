#!/usr/bin/env bash
# Runs the debug build's accessibility tour (ADR 0038) on the device adb is connected to: every
# screen, in the Light and the Dark theme, at the normal font size and at 200 %. Prints what
# fails and exits non-zero if anything does. See RELEASING.md.
#
#   tools/a11y-audit.sh               the screens of an installed, set-up MeeGone
#   tools/a11y-audit.sh --first-run   clears MeeGone's data first, so setup's screens are
#                                     checked too; for a test device, as it loses the layout
#                                     and settings
#
# Install the debug build first: (cd android && ./gradlew installDebug). The script puts the
# device's font size back as it found it; the tour puts back the theme and Recent's density.
set -euo pipefail

PACKAGE=io.github.twonet.meegone
EXTRA=io.github.twonet.meegone.debug.A11Y_AUDIT
TAG=MeeGoneA11y
SCALES=(1.0 2.0)
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

found=$(adb shell settings get system font_scale | tr -d '\r')
restore() {
  if [[ "$found" == "null" || -z "$found" ]]; then
    adb shell settings delete system font_scale >/dev/null
  else
    adb shell settings put system font_scale "$found"
  fi
}
trap restore EXIT

failures=0
for scale in "${SCALES[@]}"; do
  echo "== Font scale $scale"
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
