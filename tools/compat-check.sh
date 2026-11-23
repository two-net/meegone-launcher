#!/usr/bin/env bash
# Checks MeeGone on the device adb is connected to, for the device and navigation-mode matrix
# (COMP-01 to COMP-04, ADR 0044), with the debug build's compatibility probe. In each navigation
# mode the device offers, it checks that MeeGone reads the mode as the system sets it, that Home
# from an app lands on the home with nothing open and plays the return motion only with buttons
# (ANIM-09), that Back closes Settings and never leaves the home, and, with gestures, that a swipe
# from a side edge is Back while one across the middle changes the view. Then that turning and
# folding the device keep what is open, that each profile's apps are called work only if it is a
# work profile, and it asks whether the lock screen shows the daily image (SRS C5). Prints a
# table, and exits non-zero if any check fails. See RELEASING.md.
#
#   tools/compat-check.sh                         every check, in every mode it can reach
#   tools/compat-check.sh --mode gesture          in one mode only: three, two or gesture
#   tools/compat-check.sh --record                also writes docs/devices/<version>-<model>.md,
#                                                 or <version>-<model>-<mode>.md with --mode
#   tools/compat-check.sh --record --reference    records the device as the reference device
#   tools/compat-check.sh --lock-screen yes       the person's answer to the lock-screen question
#
# Install the debug build and finish its setup first: (cd android && ./gradlew installDebug).
# Turn the daily image on for the lock-screen question. For the run, MeeGone becomes the default
# home app, and the navigation mode and the rotation settings change; afterwards they are put
# back. On an emulator or a Pixel, adb switches the navigation mode; elsewhere the script asks
# for it to be switched in the device's settings. The device must be unlocked, and a person
# answers what adb can't reach: pausing work apps, folding a device adb can't fold, and the lock
# screen. Run without a terminal, it asks nothing, and the table marks those checks skipped.
set -euo pipefail

APP=io.github.twonet.meegone
COMPONENT="$APP/$APP.HomeActivity"
EXTRAS=$APP.debug
HOME_ROLE=android.app.role.HOME
PROBE=MeeGoneCompat
NAVBAR=com.android.internal.systemui.navbar
# Android 12L lets adb fold a device.
DEVICE_STATE_SDK=32
# Android 11 is the last with two-button navigation.
TWO_BUTTON_LAST_SDK=30
# UserInfo's flag for a work profile, on every version since Android 10.
FLAG_MANAGED_PROFILE=0x20
# A screen this wide in dp, at its narrowest, is a large screen (ADR 0009).
LARGE_SCREEN_DP=600

modes=(three two gesture)
only_mode=""
lock_answer=""
record=false
reference=false
while (($# > 0)); do
  case "$1" in
    --record) record=true ;;
    --reference) reference=true ;;
    --mode)
      [[ "${2:-}" =~ ^(three|two|gesture)$ ]] || { echo "--mode takes three, two or gesture." >&2; exit 64; }
      modes=("$2")
      only_mode=$2
      shift
      ;;
    --lock-screen)
      [[ "${2:-}" =~ ^(yes|no)$ ]] || { echo "--lock-screen takes yes or no." >&2; exit 64; }
      lock_answer=${2:0:1}
      shift
      ;;
    -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 64 ;;
  esac
  shift
done
if $reference && ! $record; then
  echo "--reference only says what --record writes." >&2
  exit 64
fi
root=$(cd "$(dirname "$0")/.." && pwd)

adb get-state >/dev/null
if ! adb shell pm list packages --user 0 "$APP" 2>/dev/null | tr -d '\r' | grep -qx "package:$APP"; then
  echo "MeeGone isn't installed: (cd android && ./gradlew installDebug)" >&2
  exit 2
fi
if ! adb shell dumpsys package "$APP" | grep -q "DEBUGGABLE"; then
  echo "The installed MeeGone isn't the debug build, which alone has the probe." >&2
  exit 2
fi
if adb shell dumpsys window | grep -q "isKeyguardShowing=true"; then
  echo "Unlock the device first." >&2
  exit 2
fi
prop() {
  adb shell getprop "$1" | tr -d '\r'
}
sdk=$(prop ro.build.version.sdk)
maker=$(prop ro.product.manufacturer)
model=$(prop ro.product.model)
emulator=false
if [[ "$(prop ro.kernel.qemu)" == 1 || "$(prop ro.hardware)" =~ ^(ranchu|goldfish)$ ]]; then
  emulator=true
fi
# Pixels and emulators switch the mode with the system's overlays; other makers keep their own
# setting beside it, which the overlays would leave out of step.
by_overlay=false
if $emulator || [[ "$maker" == [Gg]oogle ]]; then
  by_overlay=true
fi
# Whether a person can answer: without a terminal, nothing is asked and nothing is taken as done.
asking=false
if (exec </dev/tty) 2>/dev/null; then
  asking=true
fi
overlays=$(adb shell cmd overlay list 2>/dev/null | tr -d '\r' | grep -oE "$NAVBAR\.[a-z_]+" || true)

# What the run changes, to put back.
home_found=$(adb shell cmd role get-role-holders --user 0 "$HOME_ROLE" | tr -d '\r')
mode_found=$(adb shell settings get secure navigation_mode | tr -d '\r')
auto_rotation_found=$(adb shell settings get system accelerometer_rotation | tr -d '\r')
user_rotation_found=$(adb shell settings get system user_rotation | tr -d '\r')
folded_by_adb=false
# Puts back setting $2 in namespace $1 as $3 says: deleted if it had no value.
put_back() {
  if [[ "$3" == "null" || -z "$3" ]]; then
    adb shell settings delete "$1" "$2" >/dev/null
  else
    adb shell settings put "$1" "$2" "$3"
  fi
}
restore() {
  set +e
  if $folded_by_adb; then
    adb shell cmd device_state state reset >/dev/null 2>&1
  fi
  put_back system accelerometer_rotation "$auto_rotation_found"
  put_back system user_rotation "$user_rotation_found"
  if [[ "$(current_mode)" != "$mode_found" ]]; then
    if $by_overlay && [[ -n "$(overlay_of_value "$mode_found")" ]]; then
      adb shell cmd overlay enable-exclusive --category "$NAVBAR.$(overlay_of_value "$mode_found")" \
        >/dev/null 2>&1
    else
      echo "Put the navigation mode back as it was in the device's settings: $(name_of_value "$mode_found")." >&2
    fi
  fi
  adb shell am force-stop "$APP" >/dev/null 2>&1
  if [[ -n "$home_found" && "$home_found" != "$APP" ]]; then
    adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$home_found" >/dev/null 2>&1
  elif [[ -z "$home_found" ]]; then
    adb shell cmd role remove-role-holder --user 0 "$HOME_ROLE" "$APP" >/dev/null 2>&1
  fi
  adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1
}
trap restore EXIT

# The log since it was last cleared: MeeGone's own lines and the probe's.
log() {
  adb logcat -d -s "MeeGone:I" "$PROBE:I" 2>/dev/null | tr -d '\r'
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
# The value of $2 in the probe's last "$1 …" line, such as "view" in "state view=1 …"; empty if
# there is none, which no check takes for a pass.
field() {
  { log | grep "$PROBE: $1 " || true; } | tail -1 | tr ' ' '\n' | sed -n "s/^$2=//p" | head -1
}
# Asks the probe for its report and waits for it. Without the Home category, it changes nothing.
report() {
  adb logcat -c
  # It reaches the running home, which am warns of.
  adb shell am start -n "$COMPONENT" --ez "$EXTRAS.COMPAT" true >/dev/null 2>&1
  wait_for "$PROBE: done" 15
}
# The system's Home, as the home app would get it from the launcher's own icon.
go_home() {
  adb shell am start -a android.intent.action.MAIN -c android.intent.category.HOME \
    -n "$COMPONENT" >/dev/null 2>&1
  sleep 1
}
open_settings() {
  adb shell am start -a android.intent.action.APPLICATION_PREFERENCES -p "$APP" >/dev/null 2>&1
  sleep 1.5
}
home_resumed() {
  adb shell dumpsys activity activities | tr -d '\r' \
    | grep -E "topResumedActivity|mResumedActivity" | grep -q "$APP/.HomeActivity"
}
# The screen's size in px as it is turned now.
read_size() {
  local size
  size=$(adb shell dumpsys window displays | tr -d '\r' | grep -m1 -oE 'cur=[0-9]+x[0-9]+' \
    | cut -d= -f2 || true)
  [[ -n "$size" ]] || size=$(adb shell wm size | tr -d '\r' | tail -1 | awk '{ print $NF }')
  width=${size%x*}
  height=${size#*x}
}
# Asks $1, answered y or n, and prints y, n or s for skipped.
ask() {
  local answer
  if ! $asking; then
    echo s
    return
  fi
  read -r -p "$1 [y/n, s to skip] " answer </dev/tty
  case "$answer" in
    [yY]*) echo y ;;
    [nN]*) echo n ;;
    *) echo s ;;
  esac
}
# Asks for $1 to be done. Returns non-zero if it was skipped.
when_done() {
  local answer
  $asking || return 1
  read -r -p "$1 Then press Enter, or type s to skip: " answer </dev/tty
  [[ "$answer" != [sS]* ]]
}

# The navigation modes: settings' value, the system overlay's name, and what the table calls it.
value_of() {
  case "$1" in three) echo 0 ;; two) echo 1 ;; gesture) echo 2 ;; esac
}
overlay_of_value() {
  case "$1" in 0) echo threebutton ;; 1) echo twobutton ;; 2) echo gestural ;; esac
}
name_of_value() {
  case "$1" in
    0) echo "three-button" ;;
    1) echo "two-button" ;;
    2) echo "gesture" ;;
    *) echo "mode $1" ;;
  esac
}
current_mode() {
  adb shell settings get secure navigation_mode | tr -d '\r'
}
# Switches to mode $1. Returns 2 if the device doesn't offer it, 3 if no one can switch it, and 1
# if it didn't take.
switch_to() {
  local value overlay
  value=$(value_of "$1")
  [[ "$(current_mode)" == "$value" ]] && return 0
  if [[ "$1" == two ]] && ((sdk > TWO_BUTTON_LAST_SDK)); then
    return 2
  fi
  if $by_overlay; then
    overlay="$NAVBAR.$(overlay_of_value "$value")"
    grep -qxF "$overlay" <<<"$overlays" || return 2
    adb shell cmd overlay enable-exclusive --category "$overlay"
    local deadline=$((SECONDS + 10))
    until [[ "$(current_mode)" == "$value" ]] || ((SECONDS > deadline)); do
      sleep 0.5
    done
  else
    $asking || return 3
    when_done "Switch $model to $(name_of_value "$value") navigation in its settings, if it offers it." \
      || return 2
  fi
  sleep 2 # The bars settle, and the home gets their new insets.
  [[ "$(current_mode)" == "$value" ]] || return 1
}
# The system's Home or Back, in mode $1: a key with buttons, a swipe with gestures.
press_home() {
  if [[ "$1" == gesture ]]; then
    read_size
    adb shell input swipe $((width / 2)) $((height - 2)) $((width / 2)) $((height / 3)) 120
  else
    adb shell input keyevent KEYCODE_HOME
  fi
}
press_back() {
  if [[ "$1" == gesture ]]; then
    read_size
    adb shell input swipe 2 $((height / 2)) $((width / 3)) $((height / 2)) 150
  else
    adb shell input keyevent KEYCODE_BACK
  fi
}

rows=()
failures=0
# A row of the table: check $1 for requirement $2 in mode $3. $4 is yes or no, which count, or
# "skip: why" or "info: what", which don't.
row() {
  local result
  case "$4" in
    yes) result="✅" ;;
    no) result="❌"; failures=$((failures + 1)) ;;
    skip:*) result="skipped: ${4#skip: }" ;;
    info:*) result="${4#info: }" ;;
  esac
  rows+=("| $1 | $2 | $3 | $result |")
}
yes_if() {
  if "$@"; then echo yes; else echo no; fi
}

echo "== Setting up $APP on the $maker $model, API $sdk"
adb shell cmd role add-role-holder --user 0 "$HOME_ROLE" "$APP"
adb shell input keyevent KEYCODE_WAKEUP
go_home
report || { echo "The probe never reported. Is this the debug build of this commit?" >&2; exit 2; }
if [[ "$(field state setup)" != -1 ]]; then
  echo "Finish MeeGone's setup on the device first." >&2
  exit 2
fi
sw=$(field device sw)
sw=${sw:-0}
folds=$(field device folds)
class=phone
((sw >= LARGE_SCREEN_DP)) && class=tablet
[[ "$folds" == true ]] && class=foldable

ran=()
for mode in "${modes[@]}"; do
  name=$(name_of_value "$(value_of "$mode")")
  echo "== $name navigation"
  status=0
  switch_to "$mode" || status=$?
  if ((status == 2)); then
    row "Navigation mode" COMP-02 "$name" "skip: not offered"
    continue
  elif ((status == 3)); then
    row "Navigation mode" COMP-02 "$name" "skip: needs a person to switch it, with a terminal"
    continue
  elif ((status == 1)); then
    row "Navigation mode" COMP-02 "$name" no
    continue
  fi
  ran+=("$name")
  gestures=false
  [[ "$mode" == gesture ]] && gestures=true

  go_home
  report
  row "MeeGone reads the mode as the system sets it" "ANIM-09" "$name" \
    "$(yes_if [ "$(field device gestures)" == "$gestures" ])"

  adb shell am start -W -a android.settings.SETTINGS >/dev/null
  sleep 1.5
  adb logcat -c
  press_home "$mode"
  if wait_for "$PROBE: home " 8; then
    row "Home from an app lands on the home with nothing open" "HOME-06, COMP-02" "$name" \
      "$(yes_if [ "$(field home open)" == false ])"
    reading=$({ log | grep -oE "Gesture navigation: (true|false)" || true; } | tail -1 \
      | awk '{ print $NF }')
    row "The return motion plays only with buttons" "ANIM-09" "$name" \
      "$(yes_if [ "$reading" == "$gestures" ])"
  else
    row "Home from an app lands on the home with nothing open" "HOME-06, COMP-02" "$name" no
    row "The return motion plays only with buttons" "ANIM-09" "$name" "skip: no Home"
  fi

  open_settings
  report
  if [[ "$(field state settings)" != 0 ]]; then
    adb logcat -c
    press_back "$mode"
    closed=no
    wait_for "$PROBE: back " 5 && [[ "$(field back settings)" == 0 ]] && closed=yes
    row "Back closes Settings" "HOME-08, COMP-02" "$name" "$closed"
  else
    row "Back closes Settings" "HOME-08, COMP-02" "$name" "skip: Settings didn't open"
  fi
  adb logcat -c
  press_back "$mode"
  stayed=no
  wait_for "$PROBE: back " 5 && sleep 1 && home_resumed && stayed=yes
  row "Back on the home stays on the home" "HOME-08, COMP-02" "$name" "$stayed"

  # The views move under a finger dragged across the middle, in every mode.
  report
  before=$(field state view)
  read_size
  adb shell input swipe $((width * 3 / 4)) $((height / 2)) $((width / 4)) $((height / 2)) 250
  sleep 1.5
  report
  if [[ "$(field state view)" == "$before" ]]; then
    # The last view, with wrap-around off: the other way.
    adb shell input swipe $((width / 4)) $((height / 2)) $((width * 3 / 4)) $((height / 2)) 250
    sleep 1.5
    report
  fi
  row "A swipe across the middle changes the view" "HOME-13" "$name" \
    "$(yes_if [ "$(field state view)" != "$before" ])"
  if $gestures; then
    before=$(field state view)
    adb logcat -c
    press_back gesture
    edge=no
    wait_for "$PROBE: back " 5 && [[ "$(field back view)" == "$before" ]] && edge=yes
    row "A swipe from a side edge is Back and leaves the view" "HOME-13, COMP-02" "$name" "$edge"
  fi
done

echo "== Turning"
mode_name=$(name_of_value "$(current_mode)")
go_home
open_settings
report
before_view=$(field state view)
before_settings=$(field state settings)
large=false
((sw >= LARGE_SCREEN_DP)) && large=true
requested=$(field device requested)
row "Phones stay portrait; large screens turn every way" "HOME-12, ADR 0009" "$mode_name" \
  "$(yes_if [ "$([[ "$requested" == full-user ]] && echo true || echo false)" == "$large" ])"
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 1
sleep 3
report
rotation=$(field device rotation)
if $large; then
  turned=$(yes_if [ "$rotation" == 1 ])
else
  turned=$(yes_if [ "$rotation" != 1 ])
fi
row "Turned to landscape: the home follows if it is a large screen ($class)" "HOME-12" \
  "$mode_name" "$turned"
row "Turning keeps the view and Settings open" "COMP-03" "$mode_name" \
  "$(yes_if [ "$(field state view)/$(field state settings)" == "$before_view/$before_settings" ])"
adb shell settings put system user_rotation 0
sleep 2

if [[ "$folds" == true ]]; then
  echo "== Folding"
  states=$(adb shell cmd device_state print-states 2>/dev/null | tr -d '\r' || true)
  closed_state=$(grep -oE "identifier=[0-9]+, name='(CLOSED|FOLDED)'" <<<"$states" | head -1 \
    | sed -E 's/identifier=([0-9]+).*/\1/' || true)
  open_state=$(grep -oE "identifier=[0-9]+, name='(OPENED|OPEN|UNFOLDED)'" <<<"$states" | head -1 \
    | sed -E 's/identifier=([0-9]+).*/\1/' || true)
  # Folds or unfolds the device: $1 is fold or unfold. Returns non-zero if skipped.
  fold() {
    if ((sdk >= DEVICE_STATE_SDK)) && [[ -n "$closed_state" && -n "$open_state" ]]; then
      folded_by_adb=true
      if [[ "$1" == fold ]]; then
        adb shell cmd device_state state "$closed_state"
      else
        adb shell cmd device_state state "$open_state"
      fi
      sleep 3
    else
      if [[ "$1" == fold ]]; then
        when_done "Fold the device."
      else
        when_done "Unfold the device."
      fi
    fi
  }
  kept=skip
  for step in fold unfold; do
    if ! fold "$step"; then
      kept="skip: not folded"
      break
    fi
    report
    if [[ "$(field state view)/$(field state settings)" != "$before_view/$before_settings" ]]; then
      kept=no
    fi
    step_sw=$(field device sw)
    step_sw=${step_sw:-0}
    step_requested=$(field device requested)
    step_large=false
    ((step_sw >= LARGE_SCREEN_DP)) && step_large=true
    row "After ${step}ing, the orientation follows the screen ($step_sw dp)" "HOME-12, ADR 0009" \
      "$mode_name" \
      "$(yes_if [ "$([[ "$step_requested" == full-user ]] && echo true || echo false)" == "$step_large" ])"
  done
  [[ "$kept" == skip ]] && kept=yes
  row "Folding and unfolding keep the view and Settings open" "COMP-03" "$mode_name" "$kept"
  if $folded_by_adb; then
    adb shell cmd device_state state reset
    folded_by_adb=false
    sleep 2
  fi
fi
adb shell input keyevent KEYCODE_BACK

echo "== Profiles"
go_home
report
users=$(adb shell pm list users | tr -d '\r')
work_users=()
profile_names=("personal")
while read -r line; do
  [[ -n "$line" ]] || continue
  id=$(sed -nE 's/.*user=([0-9]+).*/\1/p' <<<"$line")
  [[ "$line" == *" own=true "* ]] && continue
  flags=$(sed -nE "s/.*UserInfo\{$id:[^:]*:([0-9a-fA-F]+)\}.*/\1/p" <<<"$users" | head -1)
  managed=false
  [[ -n "$flags" ]] && (((0x$flags & FLAG_MANAGED_PROFILE) != 0)) && managed=true
  work=$(sed -nE 's/.* work=(true|false).*/\1/p' <<<"$line")
  apps=$(sed -nE 's/.* apps=([0-9]+).*/\1/p' <<<"$line")
  if $managed; then
    work_users+=("$id")
    profile_names+=("work (user $id, $apps apps)")
    row "User $id, a work profile: its apps are called work" "APPS-20, A11Y-01" "—" \
      "$(yes_if [ "$work" == true ])"
  else
    profile_names+=("other (user $id, $apps apps)")
    row "User $id, not a work profile: its apps aren't called work" "APPS-20, A11Y-01" "—" \
      "$(yes_if [ "$work" == false ])"
  fi
done < <(log | grep "$PROBE: profile ")
if ((${#work_users[@]} == 0)); then
  row "Work profile" "SRS 2.5" "—" "skip: none on this device"
else
  work_user=${work_users[0]}
  paused="skip: not paused"
  offered=skip
  if when_done "Pause work apps (Quick Settings → Work apps)."; then
    report
    paused=no
    log | grep "$PROBE: profile user=$work_user " | grep -qE "paused=true|locked=true" && paused=yes
    case "$(ask "In Applications, are the work apps dimmed, and does tapping one offer to turn work apps on?")" in
      y) offered=yes ;;
      n) offered=no ;;
      *) offered="skip: not answered" ;;
    esac
    when_done "Turn work apps back on." || true
  fi
  row "Pausing work apps: MeeGone sees the profile paused" "APPS-09" "—" "$paused"
  row "Paused work apps are dimmed, and a tap offers to turn them on" "APPS-09" "—" "$offered"
fi

echo "== Lock screen"
report
unsupported=$(field lock unsupported)
lock_id=$(field lock id)
may_not_show=$(field lock may-not-show)
if [[ "$unsupported" != none ]]; then
  reading="MeeGone can't set it: $unsupported"
elif [[ "$may_not_show" == true ]]; then
  reading="MeeGone says the device may not show it (lock wallpaper $lock_id)"
else
  reading="MeeGone set it and the device kept it (lock wallpaper $lock_id)"
fi
appears=$lock_answer
[[ -n "$appears" ]] || appears=$(ask "With the daily image on, lock the device, wake it and look. Does the lock screen show the image Events' card shows? Unlock it again before answering.")
deadline=$((SECONDS + 120))
while adb shell dumpsys window | grep -q "isKeyguardShowing=true" && ((SECONDS < deadline)); do
  sleep 1
done
case "$appears" in
  y) lock_result="appears" ;;
  n) lock_result="doesn't appear" ;;
  *) lock_result="not checked" ;;
esac
row "The lock-screen image appears" "C5, WALL-01" "—" "info: $lock_result; $reading"

table=$'| Check | Requirement | Mode | Result |\n|---|---|---|---|'
for line in "${rows[@]}"; do
  table+=$'\n'"$line"
done
echo
echo "$table"

if $record; then
  version=$(adb shell dumpsys package "$APP" | tr -d '\r' | sed -n 's/^ *versionName=//p' | head -1)
  release=$(prop ro.build.version.release)
  abis=$(prop ro.product.cpu.abilist)
  density=$(adb shell wm density | tr -d '\r' | tail -1 | awk '{ print $NF }')
  read_size
  commit=$(git -C "$root" rev-parse --short HEAD)
  if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
    commit+=" with uncommitted changes"
  fi
  kind="a physical $class"
  name="${model// /-}"
  if $emulator; then
    kind="an emulated $class"
    avd=$(prop ro.boot.qemu.avd_name)
    [[ -n "$avd" ]] || avd=$(prop ro.kernel.qemu.avd_name)
    name="emulator-${avd:-$name}"
  fi
  $reference && kind+=", the reference device (#48)"
  ran_list=$(IFS=,; echo "${ran[*]:-none}")
  file="$root/docs/devices/$version-$name${only_mode:+-$only_mode}.md"
  mkdir -p "$(dirname "$file")"
  {
    echo "# MeeGone $version on the $maker $model"
    echo
    echo "Checked by \`tools/compat-check.sh\` for the device and navigation-mode matrix (ADR 0044)."
    echo
    echo "- **Date:** $(date -u +%Y-%m-%d)"
    echo "- **Commit:** $commit"
    echo "- **Build:** debug, with the compatibility probe (\`$APP\`)"
    echo "- **Device:** $maker $model, Android $release (API $sdk), ${width} × ${height} px at $density dpi, smallest width $sw dp, ABIs $abis"
    echo "- **Class:** $kind"
    echo "- **Navigation modes run:** ${ran_list//,/, }"
    echo "- **Profiles:** $(IFS=';'; echo "${profile_names[*]}" | sed 's/;/; /g')"
    echo "- **Lock-screen image:** $lock_result; $reading"
    echo
    echo "$table"
  } >"$file"
  echo "Recorded in ${file#"$root"/}"
fi

if ((failures > 0)); then
  echo "$failures checks failed." >&2
  exit 1
fi
echo "Every check passed."
