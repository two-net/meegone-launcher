#!/usr/bin/env bash
# Measures MeeGone against its performance budgets, PERF-01 to PERF-08 (SRS 6.1, ADR 0041), on
# the device adb is connected to, with the benchmark build: the release code, not debuggable,
# with the debug build's probe, installed beside MeeGone. Prints a table, and exits non-zero if
# any budget is missed. The budgets apply to the reference device (#48); see RELEASING.md.
#
#   tools/perf-bench.sh            every budget, with the issue's sample counts
#   tools/perf-bench.sh --record   also writes the table to docs/benchmarks/<version>-<model>.md
#   tools/perf-bench.sh --quick    fewer samples, to try a change; it can't be recorded
#
# Install the benchmark build first: (cd android && ./gradlew installBenchmark). For the run, it
# becomes the default home app with notification and usage access, and its catalogue is padded
# to 300 apps with copies of the installed ones. It answers its own first-run setup with the
# daily image off, so nothing is downloaded and the lock screen stays as it is. Afterwards the
# home app, the accesses and the screen timeout are put back as they were. The device must be
# unlocked, and it launches its first app and Settings many times over.
set -euo pipefail

APP=io.github.twonet.meegone.benchmark
MEEGONE=io.github.twonet.meegone
ACTIVITY=io.github.twonet.meegone.HomeActivity
COMPONENT="$APP/$ACTIVITY"
LISTENER="$APP/io.github.twonet.meegone.NotificationListener"
EXTRAS=io.github.twonet.meegone.debug
TAG=MeeGonePerf
HOME_ROLE=android.app.role.HOME
# As many apps as PERF-04, PERF-05 and PERF-07 name.
APPS=300

# The budgets (SRS 6.1).
HOME_MS=300          # PERF-01, 95th percentile
COLD_MS=1500         # PERF-02, 95th percentile
MISSED_PERCENT=1     # PERF-03, frames past their deadline
LONGEST_FRAME_MS=700 # PERF-03
LAYOUT_MS=500        # PERF-04, 95th percentile
SEARCH_MS=50         # PERF-05, 95th percentile
LAUNCH_MS=50         # PERF-06, 95th percentile
PSS_MB=150           # PERF-07
STORED_MB=50         # PERF-08
INSTALL_MB=15        # PERF-08

# Samples: AC-20's 50 Homes and 30 s of swiping.
HOMES=50
COLD_STARTS=20
FRAME_SECONDS=30
KEYSTROKES=50
LAUNCHES=20

record=false
quick=false
for arg in "$@"; do
  case "$arg" in
    --record) record=true ;;
    --quick) quick=true ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 64 ;;
  esac
done
if $quick; then
  if $record; then
    echo "--quick takes too few samples to record." >&2
    exit 64
  fi
  HOMES=10
  COLD_STARTS=5
  FRAME_SECONDS=10
  KEYSTROKES=12
  LAUNCHES=5
fi

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)

adb get-state >/dev/null
if ! adb shell pm list packages --user 0 "$APP" 2>/dev/null | tr -d '\r' | grep -qx "package:$APP"; then
  echo "The benchmark build isn't installed: (cd android && ./gradlew installBenchmark)" >&2
  exit 2
fi
if adb shell dumpsys package "$APP" | grep -q "DEBUGGABLE"; then
  echo "The installed benchmark build is debuggable, so its times would mislead." >&2
  exit 2
fi
if adb shell dumpsys window | grep -q "isKeyguardShowing=true"; then
  echo "Unlock the device first." >&2
  exit 2
fi

# What the run changes, to put back.
home_found=$(adb shell cmd role get-role-holders --user 0 "$HOME_ROLE" | tr -d '\r')
if [[ "$home_found" == "$APP" ]]; then
  # A run that was cut short left the benchmark build as the home app: MeeGone, if it's
  # installed, takes it back.
  home_found=""
  if adb shell pm list packages --user 0 "$MEEGONE" | tr -d '\r' | grep -qx "package:$MEEGONE"; then
    home_found=$MEEGONE
  else
    echo "The benchmark build is the home app already; choose another one afterwards." >&2
  fi
fi
timeout_found=$(adb shell settings get system screen_off_timeout | tr -d '\r')
restore() {
  set +e
  adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1
  if [[ -n "$home_found" && "$home_found" != "$APP" ]]; then
    adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$home_found" >/dev/null 2>&1
  else
    adb shell cmd role remove-role-holder --user 0 "$HOME_ROLE" "$APP" >/dev/null 2>&1
  fi
  adb shell cmd notification disallow_listener "$LISTENER" >/dev/null 2>&1
  adb shell appops set --user 0 "$APP" GET_USAGE_STATS default >/dev/null 2>&1
  if [[ "$timeout_found" == "null" || -z "$timeout_found" ]]; then
    adb shell settings delete system screen_off_timeout >/dev/null 2>&1
  else
    adb shell settings put system screen_off_timeout "$timeout_found" >/dev/null 2>&1
  fi
  adb shell am force-stop "$APP" >/dev/null 2>&1
  adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1
  rm -rf "$work"
}
trap restore EXIT

# The log since it was last cleared: the system's activity starts and the probe's marks, each
# line starting with its time on the monotonic clock, in seconds.
log() {
  # Some devices add a line on stderr about the kernel's log, which isn't asked for.
  adb logcat -d -v monotonic -s "$TAG:I" "ActivityTaskManager:I" 2>/dev/null | tr -d '\r'
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
# The time of the last line matching $1, or nothing.
time_of() {
  log | grep -E "$1" | tail -1 | awk '{print $1}'
}
# From time $1 to time $2, in ms.
ms() {
  awk -v from="$1" -v to="$2" 'BEGIN { printf "%d\n", (to - from) * 1000 + 0.5 }'
}
home_start="START u0 .*category.HOME.*cmp=$APP/"
# The 95th percentile of the numbers in file $1: the value 95 % of samples are at or below.
p95() {
  sort -n "$1" | awk '{ v[NR] = $1 } END { i = int(NR * 0.95); if (i < NR * 0.95) i++; print v[i] }'
}
largest() {
  sort -n "$1" | tail -1
}
count() {
  wc -l <"$1" | tr -d ' '
}
other_app() {
  adb shell am start -a android.settings.SETTINGS >/dev/null
  sleep 1.5
}
press_home() {
  adb shell input keyevent KEYCODE_HOME
}

rows=()
failures=0
# One row of the table: budget, measure, result, target, samples, whether it passed.
row() {
  local verdict="✅"
  if [[ "$6" != pass ]]; then
    verdict="❌"
    failures=$((failures + 1))
  fi
  rows+=("| $1 | $2 | $3 | $4 | $5 | $verdict |")
  echo "$1: $2: $3 (target $4) $verdict"
}
at_most() {
  awk -v value="$1" -v limit="$2" 'BEGIN { exit !(value <= limit) }' && echo pass || echo fail
}

echo "== Setting up $APP"
adb shell settings put system screen_off_timeout 1800000
adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$APP"
adb shell cmd notification allow_listener "$LISTENER"
adb shell appops set --user 0 "$APP" GET_USAGE_STATS allow
adb shell input keyevent KEYCODE_WAKEUP
# The probe's settings, then a cold start that reads 300 apps and answers setup.
adb shell am start -a android.intent.action.MAIN -c android.intent.category.HOME -n "$COMPONENT" \
  --ez "$EXTRAS.PERF" true --ei "$EXTRAS.APPS" "$APPS" >/dev/null
sleep 2
other_app
adb shell am force-stop "$APP"
adb logcat -c
press_home
wait_for "$TAG: setup-done" 30 || { echo "Setup wasn't answered." >&2; log >&2; exit 2; }
wait_for "$TAG: icons-shown" 20 || { echo "The home never showed its icons." >&2; exit 2; }
first_cell=$(log | grep "$TAG: first-cell" | tail -1 | awk '{print $(NF-1), $NF}')
# Warm up: the first Homes and swipes compile and cache what the rest use.
for _ in 1 2 3; do
  other_app
  press_home
  sleep 1
done

echo "== PERF-01: Home from an app, $HOMES times"
: >"$work/home"
for ((i = 0; i < HOMES; i++)); do
  other_app
  adb logcat -c
  press_home
  if ! wait_for "$TAG: home-ready" 5; then
    echo "No home-ready after Home $((i + 1))" >&2
    echo 99999 >>"$work/home"
    continue
  fi
  ms "$(time_of "$home_start")" "$(time_of "$TAG: home-ready")" >>"$work/home"
done
value=$(p95 "$work/home")
row PERF-01 "Home from an app to a drawn, idle home, 95th percentile" "$value ms" \
  "≤ $HOME_MS ms" "$(count "$work/home")" "$(at_most "$value" "$HOME_MS")"

echo "== PERF-02 and PERF-04: cold starts with $APPS apps, $COLD_STARTS times"
: >"$work/cold"
: >"$work/layout"
shifts=0
for ((i = 0; i < COLD_STARTS; i++)); do
  other_app
  adb shell am force-stop "$APP"
  sleep 1
  adb logcat -c
  press_home
  if ! wait_for "Fully drawn $APP/" 15 || ! wait_for "$TAG: icons-shown" 5; then
    echo "Cold start $((i + 1)) wasn't fully drawn" >&2
    echo 99999 >>"$work/cold"
    echo 99999 >>"$work/layout"
    continue
  fi
  # "Fully drawn …: +1s234ms": from the launch, as the system measures it.
  log | grep "Fully drawn $APP/" | tail -1 | sed -E 's/.*: \+//' \
    | awk -F's' '{ if (NF > 2) print $1 * 1000 + $2; else print $1 + 0 }' >>"$work/cold"
  start=$(time_of "$home_start")
  ms "$start" "$(time_of "$TAG: layout-shown")" >>"$work/layout"
  if log | grep -q "$TAG: shift"; then
    shifts=$((shifts + 1))
  fi
  sleep 2
done
stored=$(log | grep "$TAG: stored" | tail -1 | awk '{print $NF}')
value=$(p95 "$work/cold")
row PERF-02 "Cold start to Applications with its icons, 95th percentile" "$value ms" \
  "≤ $COLD_MS ms" "$(count "$work/cold")" "$(at_most "$value" "$COLD_MS")"
value=$(p95 "$work/layout")
verdict=$(at_most "$value" "$LAYOUT_MS")
if ((shifts > 0)); then
  verdict=fail
fi
row PERF-04 "Cold start to the saved layout of $APPS apps, 95th percentile; cells that moved" \
  "$value ms; $shifts" "≤ $LAYOUT_MS ms; none" "$(count "$work/layout")" "$verdict"

size=$(adb shell wm size | tr -d '\r' | sed -n 's/.*size: //p' | tail -1)
width=${size%x*}
height=${size#*x}
middle=$((height / 2))
# Frames during $1, a loop of shell commands run for FRAME_SECONDS on the device, from the home
# at rest: the share that missed their deadline, and the longest (PERF-03).
frames() {
  local name=$1 commands=$2
  press_home
  sleep 1.5
  adb shell dumpsys gfxinfo "$APP" reset >/dev/null
  adb shell "end=\$((\$(date +%s) + $FRAME_SECONDS)); while [ \$(date +%s) -lt \$end ]; do $commands; done"
  sleep 1
  adb shell dumpsys gfxinfo "$APP" | tr -d '\r' >"$work/gfx"
  local total missed longest percent
  total=$(sed -n 's/^Total frames rendered: //p' "$work/gfx" | head -1)
  missed=$(sed -n 's/^Number Frame deadline missed: //p' "$work/gfx" | head -1)
  if [[ -z "$missed" ]]; then
    # Below Android 12, janky frames are those that missed a vsync.
    missed=$(sed -n 's/^Janky frames: \([0-9]*\).*/\1/p' "$work/gfx" | head -1)
  fi
  # The histogram's last bucket with a frame in it.
  longest=$(grep -m1 '^HISTOGRAM:' "$work/gfx" | tr ' ' '\n' | grep '=' \
    | awk -F'[=m]' '$3 > 0 { last = $1 } END { print last + 0 }')
  percent=$(awk -v m="$missed" -v t="$total" 'BEGIN { printf "%.2f", (t > 0 ? 100 * m / t : 0) }')
  local verdict=pass
  if [[ "$(at_most "$percent" "$MISSED_PERCENT")" != pass ]] || ((longest > LONGEST_FRAME_MS)); then
    verdict=fail
  fi
  row PERF-03 "$name: frames past their deadline; longest" "$percent % ($missed of $total); $longest ms" \
    "≤ $MISSED_PERCENT %; ≤ $LONGEST_FRAME_MS ms" "${FRAME_SECONDS} s" "$verdict"
}

echo "== PERF-03: frames"
left=$((width * 15 / 100))
right=$((width * 85 / 100))
next="input swipe $right $middle $left $middle 250"
previous="input swipe $left $middle $right $middle 250"
frames "Swiping between the views" "$next; $previous; $previous; $next"
x=$((width / 2))
low=$((height * 75 / 100))
high=$((height * 30 / 100))
frames "Scrolling Applications" \
  "input swipe $x $low $x $high 400; input swipe $x $low $x $high 400; input swipe $x $high $x $low 400"
frames "Menus, search and Recent's close mode by key" \
  "input keyevent KEYCODE_MENU; sleep 0.6; input keyevent KEYCODE_ESCAPE; sleep 0.6; input text m; sleep 0.8; input keyevent KEYCODE_ESCAPE; sleep 0.4; input keyevent KEYCODE_ESCAPE; sleep 0.6; $next; sleep 0.6; input keyevent KEYCODE_MENU; sleep 0.6; input keyevent KEYCODE_ESCAPE; sleep 0.6; $previous; sleep 0.6"

echo "== PERF-05: $KEYSTROKES keystrokes in search"
press_home
sleep 1.5
keys=(M A P S DEL DEL DEL DEL C A M E R A DEL DEL DEL DEL DEL DEL C L O C K DEL DEL DEL DEL DEL)
typed=""
for ((i = 0; i < KEYSTROKES - 1; i++)); do
  typed+="input keyevent KEYCODE_${keys[i % ${#keys[@]}]}; sleep 0.25; "
done
adb logcat -c
# The first letter, typed on Applications, opens search with it (A11Y-06).
adb shell "input text m; sleep 0.8; $typed"
sleep 1
log | grep "$TAG: search " | awk '{print $NF}' >"$work/search"
press_home
# The keyboard goes, and the home comes to rest, before the next test.
sleep 3
value=$(p95 "$work/search")
row PERF-05 "A keystroke to its results drawn, 95th percentile; longest" \
  "$value ms; $(largest "$work/search") ms" "≤ $SEARCH_MS ms" "$(count "$work/search")" \
  "$(at_most "$value" "$SEARCH_MS")"

echo "== PERF-06: $LAUNCHES launches from the grid"
: >"$work/launch"
for ((i = 0; i < LAUNCHES; i++)); do
  press_home
  sleep 1.5
  adb logcat -c
  # shellcheck disable=SC2086 # Two coordinates.
  adb shell input tap $first_cell
  if wait_for "$TAG: launch " 5; then
    log | grep "$TAG: launch " | tail -1 | awk '{print $NF}' >>"$work/launch"
  else
    echo "No launch after tap $((i + 1))" >&2
    echo 99999 >>"$work/launch"
  fi
  sleep 1.5
done
value=$(p95 "$work/launch")
row PERF-06 "The finger lifting to the launch request leaving, 95th percentile" "$value ms" \
  "≤ $LAUNCH_MS ms" "$(count "$work/launch")" "$(at_most "$value" "$LAUNCH_MS")"

echo "== PERF-07: memory with $APPS apps"
# The median of three readings, as a reading can catch a passing spike.
pss() {
  for _ in 1 2 3; do
    adb shell dumpsys meminfo "$APP" | tr -d '\r' | awk '$1 == "TOTAL" && $2 ~ /^[0-9]+$/ { print $2; exit }'
    sleep 2
  done | sort -n | sed -n 2p
}
other_app
adb logcat -c
press_home
wait_for "$TAG: home-ready" 5 || true
icons_before=$(log | grep "$TAG: home-ready" | tail -1 | sed 's/.*icons=//')
sleep 10
before=$(pss)
adb logcat -c
adb shell am send-trim-memory "$APP" RUNNING_CRITICAL
sleep 5
icons_after=$(log | grep "$TAG: trim " | tail -1 | sed 's/.*icons=//')
after=$(pss)
before_mb=$(awk -v kb="$before" 'BEGIN { printf "%.0f", kb / 1024 }')
after_mb=$(awk -v kb="$after" 'BEGIN { printf "%.0f", kb / 1024 }')
verdict=$(at_most "$before_mb" "$PSS_MB")
if [[ -z "$icons_after" || -z "$icons_before" ]] || ((icons_after >= icons_before && icons_before > 0)); then
  verdict=fail
fi
row PERF-07 "Total PSS at rest; after the system asks for memory back; icons held" \
  "$before_mb MB; $after_mb MB; ${icons_before:-?} → ${icons_after:-?}" \
  "≤ $PSS_MB MB; caches shrink" "3 readings" "$verdict"

echo "== PERF-08: storage and install size"
apk=$(adb shell pm path --user 0 "$APP" | tr -d '\r' | sed -n 's/^package://p' | head -1)
apk_bytes=$(adb shell stat -c %s "$apk" | tr -d '\r')
stored_mb=$(awk -v b="${stored:-0}" 'BEGIN { printf "%.2f", b / 1048576 }')
apk_mb=$(awk -v b="$apk_bytes" 'BEGIN { printf "%.1f", b / 1048576 }')
verdict=$(at_most "$stored_mb" "$STORED_MB")
if [[ -z "$stored" || "$(at_most "$apk_mb" "$INSTALL_MB")" != pass ]]; then
  verdict=fail
fi
row PERF-08 "Stored data; the installed APK" "$stored_mb MB; $apk_mb MB" \
  "≤ $STORED_MB MB; ≤ $INSTALL_MB MB" "1" "$verdict"

table=$'| Budget | Measure | Result | Target | Samples | |\n|---|---|---|---|---|---|'
for line in "${rows[@]}"; do
  table+=$'\n'"$line"
done
echo
echo "$table"

if $record; then
  version=$(adb shell dumpsys package "$APP" | tr -d '\r' | sed -n 's/^ *versionName=//p' | head -1)
  maker=$(adb shell getprop ro.product.manufacturer | tr -d '\r')
  model=$(adb shell getprop ro.product.model | tr -d '\r')
  release=$(adb shell getprop ro.build.version.release | tr -d '\r')
  sdk=$(adb shell getprop ro.build.version.sdk | tr -d '\r')
  rate=$(adb shell dumpsys display | tr -d '\r' | grep -m1 -oE 'renderFrameRate [0-9.]+' \
    | awk '{ printf "%.0f", $2 }' || true)
  navigation=$(adb shell settings get secure navigation_mode | tr -d '\r')
  case "$navigation" in
    0) navigation="three-button navigation" ;;
    1) navigation="two-button navigation" ;;
    2) navigation="gesture navigation" ;;
    *) navigation="navigation mode $navigation" ;;
  esac
  installed=$(adb shell cmd package query-activities --user 0 --brief \
    -a android.intent.action.MAIN -c android.intent.category.LAUNCHER | tr -d '\r' \
    | grep -c '/' || true)
  commit=$(git -C "$root" rev-parse --short HEAD)
  if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
    commit+=" with uncommitted changes"
  fi
  file="$root/docs/benchmarks/$version-${model// /-}.md"
  mkdir -p "$(dirname "$file")"
  {
    echo "# MeeGone $version on the $maker $model"
    echo
    echo "Measured by \`tools/perf-bench.sh\` against SRS 6.1's budgets (ADR 0041)."
    echo
    echo "- **Date:** $(date -u +%Y-%m-%d)"
    echo "- **Commit:** $commit"
    echo "- **Build:** benchmark: the release code, not debuggable, with the probe (\`$APP\`)"
    echo "- **Device:** $maker $model, Android $release (API $sdk), ${size/x/ × } px at ${rate:-?} Hz, $navigation"
    echo "- **Apps:** $installed with a launcher entry, padded to $APPS with copies of them"
    echo
    echo "$table"
  } >"$file"
  echo "Recorded in ${file#"$root"/}"
fi

if ((failures > 0)); then
  echo "$failures budgets were missed." >&2
  exit 1
fi
echo "Every budget was met."
