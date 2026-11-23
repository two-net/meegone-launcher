#!/usr/bin/env bash
# Checks on the device adb is connected to that a part of MeeGone that fails leaves the rest
# working (REL-01, ARCH-09, ADR 0043), with the debug build, whose Rust app makes bugs to order.
# For each fault, MeeGone starts with it armed, and the script checks that the part says it
# stopped working, that the process lives on with no crash recorded, and that an app still opens
# from Applications. Then, once, that Restart MeeGone starts it again in a new process, which
# Android records as the app exiting itself, not as a crash (REL-07). Prints a table, and exits
# non-zero if any check fails. See RELEASING.md.
#
#   tools/fault-check.sh          every fault, in the Java views and in the Rust app
#   tools/fault-check.sh --java   only the faults in the Java views
#   tools/fault-check.sh --rust   only the faults in the Rust app, and the restart
#
# Install the debug build first: (cd android && ./gradlew installDebug). For the run it becomes
# the default home app with notification and usage access, and its probe answers setup with the
# daily image off; afterwards the home app and the accesses are put back. The device must be
# unlocked, with English as its language, for the Restart button's text, and it launches its
# first app once for each fault.
set -euo pipefail

APP=io.github.twonet.meegone
COMPONENT="$APP/$APP.HomeActivity"
LISTENER="$APP/$APP.NotificationListener"
EXTRAS=$APP.debug
HOME_ROLE=android.app.role.HOME
PROBE=MeeGonePerf
RESTART_TEXT="Restart MeeGone"
# Android 11 records why each process ended.
EXIT_INFO_SDK=30

# Each fault: a part, and where it fails (DebugHooks.EXTRA_FAULT).
JAVA_FAULTS=(events-java recent-java image-java settings-java)
RUST_FAULTS=(applications-rust events-rust recent-rust image-rust settings-rust events-reader
  recent-reader)

faults=("${JAVA_FAULTS[@]}" "${RUST_FAULTS[@]}")
restart=true
for arg in "$@"; do
  case "$arg" in
    --java) faults=("${JAVA_FAULTS[@]}"); restart=false ;;
    --rust) faults=("${RUST_FAULTS[@]}") ;;
    -h|--help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 64 ;;
  esac
done

adb get-state >/dev/null
if ! adb shell pm list packages --user 0 "$APP" 2>/dev/null | tr -d '\r' | grep -qx "package:$APP"; then
  echo "MeeGone isn't installed: (cd android && ./gradlew installDebug)" >&2
  exit 2
fi
if ! adb shell dumpsys package "$APP" | grep -q "DEBUGGABLE"; then
  echo "The installed MeeGone isn't the debug build, which alone makes bugs to order." >&2
  exit 2
fi
if adb shell dumpsys window | grep -q "isKeyguardShowing=true"; then
  echo "Unlock the device first." >&2
  exit 2
fi
sdk=$(adb shell getprop ro.build.version.sdk | tr -d '\r')

# What the run changes, to put back.
home_found=$(adb shell cmd role get-role-holders --user 0 "$HOME_ROLE" | tr -d '\r')
restore() {
  set +e
  adb shell am start -n "$COMPONENT" --ez "$EXTRAS.PERF" false >/dev/null 2>&1
  adb shell am force-stop "$APP" >/dev/null 2>&1
  if [[ -n "$home_found" && "$home_found" != "$APP" ]]; then
    adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$home_found" >/dev/null 2>&1
  elif [[ -z "$home_found" ]]; then
    adb shell cmd role remove-role-holder --user 0 "$HOME_ROLE" "$APP" >/dev/null 2>&1
  fi
  adb shell cmd notification disallow_listener "$LISTENER" >/dev/null 2>&1
  adb shell appops set --user 0 "$APP" GET_USAGE_STATS default >/dev/null 2>&1
  adb shell rm -f /sdcard/fault-check.xml >/dev/null 2>&1
  adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1
}
trap restore EXIT

# The log since it was last cleared: MeeGone's own lines and the probe's marks.
log() {
  adb logcat -d -s "MeeGone:*" "$PROBE:I" 2>/dev/null | tr -d '\r'
}
# Waits up to $2 seconds for a log line matching $1. Returns non-zero if none came.
wait_for() {
  local deadline=$((SECONDS + $2))
  until log | grep -qE "$1"; do
    if ((SECONDS > deadline)); then
      return 1
    fi
    sleep 0.3
  done
}
pid() {
  adb shell pidof "$APP" 2>/dev/null | tr -d '\r' || true
}
# How many crashes the system has recorded for MeeGone, in its crash log and its drop box.
crashes() {
  local logged recorded
  logged=$(adb logcat -d -b crash 2>/dev/null | grep -c "$APP" || true)
  recorded=$(adb shell dumpsys dropbox --print data_app_crash data_app_native_crash 2>/dev/null \
    | grep -c "Process: $APP" || true)
  echo $((logged + recorded))
}
# Starts MeeGone afresh as the home, with the faults $1 armed if there are any, and waits for
# its icons.
start() {
  # adb drops an empty argument, which would leave --es without its value.
  local fault=()
  if [[ -n "$1" ]]; then
    fault=(--es "$EXTRAS.FAULT" "$1")
  fi
  adb shell am force-stop "$APP"
  adb logcat -c
  adb logcat -b crash -c 2>/dev/null || true
  adb shell am start -a android.intent.action.MAIN -c android.intent.category.HOME \
    -n "$COMPONENT" --ez "$EXTRAS.PERF" true ${fault[@]+"${fault[@]}"} >/dev/null
  wait_for "$PROBE: icons-shown" 20
}
# Goes home and taps the first cell. Returns non-zero if no launch followed.
launch_first() {
  adb shell input keyevent KEYCODE_HOME
  sleep 1.5
  local cell
  cell=$(log | grep "$PROBE: first-cell" | tail -1 | awk '{print $(NF-1), $NF}')
  [[ -n "$cell" ]] || return 1
  adb logcat -c
  # shellcheck disable=SC2086 # Two coordinates.
  adb shell input tap $cell
  wait_for "$PROBE: launch " 5
}

rows=()
failures=0
# ✅ for yes, otherwise ❌.
mark() {
  if [[ "$1" == yes ]]; then echo "✅"; else echo "❌"; fi
}
# Counts every result that isn't yes as a failure.
tally() {
  local result
  for result in "$@"; do
    [[ "$result" == yes ]] || failures=$((failures + 1))
  done
}

echo "== Setting up $APP"
adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$APP"
adb shell cmd notification allow_listener "$LISTENER"
adb shell appops set --user 0 "$APP" GET_USAGE_STATS allow
adb shell input keyevent KEYCODE_WAKEUP
# A first start, whose probe answers setup if it shows.
start "" || { echo "MeeGone never showed its icons." >&2; exit 2; }
crashes_before=$(crashes)

for fault in "${faults[@]}"; do
  echo "== $fault"
  if ! start "$fault"; then
    rows+=("| $fault | ❌ never showed its icons | | | |")
    failures=$((failures + 1))
    continue
  fi
  before=$(pid)
  stopped=no
  wait_for "Feature [0-9] stopped working" 10 && stopped=yes
  sleep 1
  alive=no
  [[ -n "$before" && "$(pid)" == "$before" ]] && alive=yes
  launched=no
  launch_first && launched=yes
  crashed=no
  (($(crashes) == crashes_before)) || crashed=yes
  clean=no
  [[ "$crashed" == no ]] && clean=yes
  tally "$stopped" "$alive" "$clean" "$launched"
  rows+=("| $fault | $(mark "$stopped") | $(mark "$alive") | $(mark "$clean") | $(mark "$launched") |")
  crashes_before=$(crashes)
done

restarted="not checked"
if $restart; then
  echo "== Restart MeeGone"
  restarted=no
  # The fault is armed on the running home, not in the intent that starts it: Android brings
  # the home back after the restart with that intent, and the new process must start clean.
  # Applications' next step fails: Home, which scrolls it to the top.
  if start "" \
    && adb shell am start -a android.intent.action.MAIN -c android.intent.category.HOME \
      -n "$COMPONENT" --es "$EXTRAS.FAULT" applications-rust >/dev/null \
    && sleep 1 && adb shell input keyevent KEYCODE_HOME \
    && wait_for "Feature 0 stopped working" 10; then
    before=$(pid)
    sleep 1.5
    adb shell uiautomator dump /sdcard/fault-check.xml >/dev/null
    bounds=$(adb shell cat /sdcard/fault-check.xml | tr '>' '\n' \
      | grep "text=\"$RESTART_TEXT\"" | head -1 \
      | sed -E 's/.*bounds="\[([0-9]+),([0-9]+)\]\[([0-9]+),([0-9]+)\]".*/\1 \2 \3 \4/')
    if [[ -n "$bounds" ]]; then
      read -r left top right bottom <<<"$bounds"
      adb logcat -c
      adb shell input tap $(((left + right) / 2)) $(((top + bottom) / 2))
      deadline=$((SECONDS + 10))
      after=""
      until [[ -n "$after" && "$after" != "$before" ]] || ((SECONDS > deadline)); do
        sleep 0.5
        after=$(pid)
      done
      resumed=no
      adb shell dumpsys activity activities | grep -E "topResumedActivity|mResumedActivity" \
        | grep -q "$APP/.HomeActivity" && resumed=yes
      exited_self=yes
      if ((sdk >= EXIT_INFO_SDK)); then
        adb shell dumpsys activity exit-info "$APP" | tr -d '\r' \
          | grep -m1 -A3 "pid=$before" | grep -q "reason=1 (EXIT_SELF)" || exited_self=no
      else
        echo "Android $sdk records no exit reasons; that check is skipped." >&2
      fi
      # The new process starts with every part working.
      fresh=no
      if wait_for "$PROBE: icons-shown" 20 && ! log | grep -q "stopped working"; then
        fresh=yes
      fi
      if [[ -n "$after" && "$after" != "$before" && "$resumed" == yes && "$exited_self" == yes \
        && "$fresh" == yes ]] && (($(crashes) == crashes_before)) && launch_first; then
        restarted=yes
      fi
    else
      echo "No \"$RESTART_TEXT\" on screen: is the device's language English?" >&2
    fi
  fi
  tally "$restarted"
fi

echo
echo "| Fault | Said it stopped | Process lived on | No crash recorded | An app opened |"
echo "|---|---|---|---|---|"
printf '%s\n' "${rows[@]}"
if [[ "$restarted" != "not checked" ]]; then
  echo
  echo "Restart MeeGone: a new process with every part working, exiting itself, then an app" \
    "opened: $(mark "$restarted")"
fi
exit $((failures > 0))
