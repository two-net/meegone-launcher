#!/usr/bin/env bash
# Checks where MeeGone's traffic goes over a day of use (PRIV-02, SEC-01, ADR 0042): only to the
# image feed's host, www.bing.com, and only over HTTPS. PCAPdroid captures the release build's
# traffic alone on the device adb is connected to, as a local VPN, without root; tshark reads
# the capture here. Prints a table of every destination, and exits non-zero if any isn't the
# feed's. See RELEASING.md.
#
#   tools/net-audit.sh --start          starts capturing MeeGone's traffic, and returns
#   tools/net-audit.sh --stop           stops, pulls the capture and checks it
#   tools/net-audit.sh --stop --record  also writes the table to docs/privacy/<version>-<model>.md
#   tools/net-audit.sh --check FILE     checks a pcap of MeeGone's traffic alone, made another way
#
# Needs the release build installed, PCAPdroid 1.8.6 or later on the device, and Wireshark's
# tshark here. Set PCAPDROID_API_KEY to the key on PCAPdroid's Control permissions page, or
# confirm its prompt on the device at the start and the stop; the first capture also asks to
# allow its VPN. Between --start and --stop, at least a day apart, use the device as usual with
# the daily image on: let the daily job run, open the details and More info, browse the past
# week, pin an image and unpin it, and tap Update now. Links open in the browser, whose traffic
# isn't MeeGone's and isn't captured. Only TCP and UDP are read.
set -euo pipefail

APP=io.github.twonet.meegone
PCAPDROID=com.emanuelef.remote_capture
CONTROL="$PCAPDROID/.activities.CaptureCtrl"
# PCAPdroid writes the capture to the device's Download/PCAPdroid folder.
PCAP=meegone-net-audit.pcap
DEVICE_PCAP="/sdcard/Download/PCAPdroid/$PCAP"
# When the capture started, kept on the device for --stop.
STARTED=/data/local/tmp/meegone-net-audit-started
# The image feed's one host (PRIV-02, ADR 0027), and the least a recorded capture covers.
ALLOWED_HOST=www.bing.com
DAY_HOURS=24

mode=""
file=""
record=false
while (($# > 0)); do
  case "$1" in
    --start) mode=start ;;
    --stop) mode=stop ;;
    --check)
      mode=check
      file=${2:-}
      if [[ -z "$file" ]]; then
        echo "--check needs the capture's file." >&2
        exit 64
      fi
      shift
      ;;
    --record) record=true ;;
    -h|--help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 64 ;;
  esac
  shift
done
if [[ -z "$mode" ]]; then
  echo "Say --start, --stop or --check FILE; --help explains." >&2
  exit 64
fi
if $record && [[ "$mode" != stop ]]; then
  echo "Only --stop can be recorded: it knows the device and how long the capture ran." >&2
  exit 64
fi

root=$(cd "$(dirname "$0")/.." && pwd)

# Asks PCAPdroid's control activity to `action`, with any further extras.
pcapdroid() {
  local action=$1
  shift
  local key=()
  if [[ -n "${PCAPDROID_API_KEY:-}" ]]; then
    key=(-e api_key "$PCAPDROID_API_KEY")
  fi
  adb shell am start -n "$CONTROL" -e action "$action" ${key[@]+"${key[@]}"} "$@" >/dev/null
}

device_ready() {
  adb get-state >/dev/null
  if ! adb shell pm list packages --user 0 "$APP" | tr -d '\r' | grep -qx "package:$APP"; then
    echo "MeeGone's release build isn't installed." >&2
    exit 2
  fi
  if adb shell dumpsys package "$APP" | grep -q DEBUGGABLE; then
    echo "The installed MeeGone is debuggable: install the release build." >&2
    exit 2
  fi
  if ! adb shell pm list packages --user 0 "$PCAPDROID" | tr -d '\r' \
    | grep -qx "package:$PCAPDROID"; then
    echo "PCAPdroid isn't installed on the device." >&2
    exit 2
  fi
}

tshark_ready() {
  if ! command -v tshark >/dev/null; then
    echo "tshark isn't installed: it comes with Wireshark." >&2
    exit 2
  fi
}

if [[ "$mode" == start ]]; then
  device_ready
  adb shell rm -f "$DEVICE_PCAP"
  adb shell "date +%s > $STARTED"
  pcapdroid start -e pcap_dump_mode pcap_file -e pcap_name "$PCAP" -e app_filter "$APP"
  echo "PCAPdroid is capturing MeeGone's traffic; confirm on the device if it asks."
  echo "Use the device for a day, then run: tools/net-audit.sh --stop --record"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tshark_ready
hours=""
if [[ "$mode" == stop ]]; then
  device_ready
  pcapdroid stop
  # PCAPdroid closes the file as the capture stops.
  sleep 3
  started=$(adb shell cat "$STARTED" 2>/dev/null | tr -d '\r' || true)
  if [[ "$started" =~ ^[0-9]+$ ]]; then
    hours=$(( ($(adb shell date +%s | tr -d '\r') - started) / 3600 ))
  fi
  if ! adb pull "$DEVICE_PCAP" "$work/$PCAP" >/dev/null; then
    echo "No capture at $DEVICE_PCAP: did --start's capture run?" >&2
    exit 2
  fi
  file="$work/$PCAP"
elif [[ ! -f "$file" ]]; then
  echo "$file isn't a file." >&2
  exit 2
fi

# Every TCP and UDP packet: its stream, addresses, ports, length, and the name it carries, from
# a TLS client hello's server name or a DNS query.
tshark -r "$file" -n -T fields -E separator=/t -E occurrence=f \
  -e tcp.stream -e udp.stream -e ip.src -e ipv6.src -e ip.dst -e ipv6.dst \
  -e tcp.srcport -e tcp.dstport -e udp.srcport -e udp.dstport -e frame.len \
  -e tls.handshake.extensions_server_name -e dns.qry.name >"$work/packets.tsv"
packets=$(wc -l <"$work/packets.tsv" | tr -d ' ')

# One line per destination: verdict, name, protocol and port, connections, kilobytes. A stream's
# server is the end with the lower port. It is allowed only as the feed's host: over TLS on
# TCP 443 with that server name, or a DNS query for it.
awk -F'\t' -v allowed="$ALLOWED_HOST" '
  {
    if ($1 != "") { key = "tcp " $1; proto = "TCP"; a = $7; b = $8 }
    else if ($2 != "") { key = "udp " $2; proto = "UDP"; a = $9; b = $10 }
    else next
    src = ($3 != "") ? $3 : $4
    dst = ($5 != "") ? $5 : $6
    if (!(key in bytes)) order[++streams] = key
    if (a + 0 < b + 0) { server[key] = src; port[key] = proto " " a }
    else { server[key] = dst; port[key] = proto " " b }
    bytes[key] += $11
    if ($12 != "") name[key] = $12
    if ($13 != "" && name[key] == "") name[key] = $13
  }
  END {
    for (i = 1; i <= streams; i++) {
      k = order[i]
      shown = (name[k] != "") ? name[k] : "no name: " server[k]
      row = shown "\t" port[k]
      if (!(row in count)) rows[++n] = row
      count[row]++
      size[row] += bytes[k]
      ok = (name[k] == allowed && (port[k] == "TCP 443" || port[k] == "UDP 53"))
      if (!ok) bad[row] = 1
    }
    for (i = 1; i <= n; i++) {
      r = rows[i]
      printf "%s\t%s\t%d\t%.1f\n", (r in bad) ? "fail" : "pass", r, count[r], size[r] / 1000
    }
  }
' "$work/packets.tsv" >"$work/destinations.tsv"

table=$'| Destination | Protocol and port | Connections | Data | |\n|---|---|---|---|---|'
failures=0
while IFS=$'\t' read -r verdict destination port connections kilobytes; do
  mark="✅"
  if [[ "$verdict" != pass ]]; then
    mark="❌"
    failures=$((failures + 1))
  fi
  table+=$'\n'"| $destination | $port | $connections | $kilobytes kB | $mark |"
done <"$work/destinations.tsv"
echo "$table"
echo
echo "$packets packets${hours:+ over $hours hours}."

if [[ ! -s "$work/destinations.tsv" ]]; then
  echo "The capture holds no traffic of MeeGone's: was it capturing, with the daily image on?" >&2
  exit 1
fi

if $record; then
  if [[ -z "$hours" || "$hours" -lt "$DAY_HOURS" ]]; then
    echo "The capture covered ${hours:-an unknown number of} hours, less than a day: not recorded." >&2
    exit 1
  fi
  version=$(adb shell dumpsys package "$APP" | tr -d '\r' | sed -n 's/^ *versionName=//p' | head -1)
  maker=$(adb shell getprop ro.product.manufacturer | tr -d '\r')
  model=$(adb shell getprop ro.product.model | tr -d '\r')
  release=$(adb shell getprop ro.build.version.release | tr -d '\r')
  sdk=$(adb shell getprop ro.build.version.sdk | tr -d '\r')
  commit=$(git -C "$root" rev-parse --short HEAD)
  if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
    commit+=" with uncommitted changes"
  fi
  out="$root/docs/privacy/$version-${model// /-}.md"
  mkdir -p "$(dirname "$out")"
  {
    echo "# MeeGone $version on the $maker $model: where its traffic went"
    echo
    echo "Captured by \`tools/net-audit.sh\` with PCAPdroid over a day of use, against PRIV-02 and SEC-01 (ADR 0042)."
    echo
    echo "- **Date:** $(date -u +%Y-%m-%d), after $hours hours"
    echo "- **Commit:** $commit"
    echo "- **Build:** release (\`$APP\`)"
    echo "- **Device:** $maker $model, Android $release (API $sdk)"
    echo "- **Packets:** $packets, MeeGone's alone"
    echo
    echo "$table"
  } >"$out"
  echo "Recorded in ${out#"$root"/}"
fi

if ((failures > 0)); then
  echo "$failures destinations aren't the image feed's host over HTTPS." >&2
  exit 1
fi
echo "Every connection went to $ALLOWED_HOST."
