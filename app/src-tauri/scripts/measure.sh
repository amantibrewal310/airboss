#!/bin/sh
# Measures a release build against SPEC-app's targets, on a throwaway database:
#   download size (the .dmg), cold start (median of N launches, process start to ui_ready),
#   the hotkey path (second launch with --quick-add to quickadd_shown),
#   idle memory (the app plus its WebKit processes, both web views alive) and idle CPU.
# Prints the machine too. Never touches ~/Library/Application Support/Airboss.
#
#   app/src-tauri/scripts/measure.sh [path/to/Airboss.app] [path/to/.dmg]
#
# Build first: (cd app && CI=true pnpm tauri build --config src-tauri/tauri.bundle.conf.json --bundles app,dmg)
set -eu

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
APP="${1:-$ROOT/target/release/bundle/macos/Airboss.app}"
DMG="${2:-$(ls "$ROOT"/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1)}"
BIN="$APP/Contents/MacOS/airboss-app"
LOG="$HOME/Library/Logs/Airboss/app.log"
STATE="$HOME/Library/Saved Application State/io.github.amantibrewal310.airboss.savedState"
RUNS="${RUNS:-10}"

WORK="$(mktemp -d)"
export AIRBOSS_DB="$WORK/airboss.db"
trap 'pkill -f "$BIN" 2>/dev/null || true; rm -rf "$WORK"' EXIT

now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }

echo "machine: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.memsize | awk '{print $1/1073741824 " GB"}'), macOS $(sw_vers -productVersion)"
echo "app:     $APP"

# --- size
if [ -n "$DMG" ] && [ -f "$DMG" ]; then
  echo "dmg:     $(du -k "$DMG" | awk '{printf "%.2f MB", $1/1024}') ($(basename "$DMG"))"
fi
echo "bundle:  $(du -sk "$APP" | awk '{printf "%.2f MB", $1/1024}') (main binary $(du -k "$BIN" | awk '{printf "%.2f MB", $1/1024}'), airboss CLI $(du -k "$APP/Contents/MacOS/airboss" | awk '{printf "%.2f MB", $1/1024}'))"

# --- data: 2,000 tasks and 1,000 dependencies
(cd "$ROOT" && cargo run -q --release -p airboss-app --example seed -- --tasks 2000 --deps 1000 --db "$AIRBOSS_DB")

# Launched through LaunchServices, as from Finder or the Dock, so macOS attributes the web
# views' WebKit processes to the app. `$1` is an extra argument (--hidden).
start_app() {
  rm -rf "$STATE"
  open -n --env "AIRBOSS_DB=$AIRBOSS_DB" "$APP" --args ${1:-} >/dev/null 2>&1
  for _ in $(seq 1 50); do
    p=$(pgrep -n -f "$BIN" || true)
    [ -n "$p" ] && { echo "$p"; return; }
    sleep 0.05
  done
}

stop_app() {
  kill -TERM "$1" 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$1" 2>/dev/null || return 0; sleep 0.3; done
  kill -KILL "$1" 2>/dev/null || true
  rm -rf "$STATE"
}

# --- cold start: from launch to the UI's ui_ready (logged with epoch_ms)
times=""
i=0
while [ $i -lt "$RUNS" ]; do
  lines=$(wc -l < "$LOG" 2>/dev/null || echo 0)
  t0=$(now_ms)
  pid=$(start_app)
  ready=""
  for _ in $(seq 1 100); do
    ready=$(tail -n +"$((lines + 1))" "$LOG" 2>/dev/null | grep -o 'ui_ready after [0-9]* ms; epoch_ms=[0-9]*' | tail -1 | sed 's/.*epoch_ms=//')
    [ -n "$ready" ] && break
    sleep 0.1
  done
  if [ -n "$ready" ]; then times="$times $((ready - t0))"; fi
  stop_app "$pid"
  sleep 1
  i=$((i + 1))
done
echo "cold start (launch to ui_ready, $RUNS runs, ms):$times"
echo "$times" | tr ' ' '\n' | grep -v '^$' | sort -n | awk '{a[NR]=$1} END {if (NR) printf "cold start median: %d ms\n", (NR%2 ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2)}'

# --- the long-running instance for memory, CPU and quick add
lines=$(wc -l < "$LOG" 2>/dev/null || echo 0)
pid=$(start_app)
sleep 4

# Quick add: open and hide it 20 times through a second launch (the hotkey's path).
for _ in $(seq 1 20); do
  "$BIN" --quick-add >/dev/null 2>&1; sleep 0.4
  "$BIN" --quick-add >/dev/null 2>&1; sleep 0.4
done
tail -n +"$((lines + 1))" "$LOG" | grep -o 'quickadd shown [0-9]* ms' | awk '{print $3}' | sort -n |
  awk '{a[NR]=$1} END {if (NR) printf "quick add (trigger to quickadd_shown, %d presses): median %d ms, p95 %d ms\n", NR, a[int((NR+1)/2)], a[int(NR*0.95+0.5)]}'

# Memory with both web views alive: the app and every WebKit process it's responsible for
# (macOS's own attribution, as Activity Monitor uses; no root needed).
cat > "$WORK/resp.c" <<'C'
#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
extern pid_t responsibility_get_pid_responsible_for_pid(pid_t);
int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) printf("%s %d\n", argv[i], responsibility_get_pid_responsible_for_pid(atoi(argv[i])));
  return 0;
}
C
clang -O2 -o "$WORK/resp" "$WORK/resp.c"
footprint_kb() {
  footprint "$1" 2>/dev/null | awk '/Footprint:/ {for (i=1;i<=NF;i++) if ($i=="Footprint:") {v=$(i+1); u=$(i+2)}; if (u=="MB") v*=1024; if (u=="GB") v*=1048576; print int(v); exit}'
}
memory() {
  total=0
  for p in "$pid" $("$WORK/resp" $(pgrep -f 'com.apple.WebKit\.' || true) | awk -v app="$pid" '$2==app {print $1}'); do
    kb=$(footprint_kb "$p")
    [ -n "$kb" ] || continue
    total=$((total + kb))
    echo "  $(ps -o comm= -p "$p" | xargs basename): $((kb / 1024)) MB"
  done
  echo "idle memory, $1 (app + its WebKit processes): $((total / 1024)) MB"
}
sleep 60
memory "main window shown, quick add opened once"

# Idle CPU over 60 s, window visible.
cpu() {
  top -l 31 -s 2 -pid "$pid" -stats pid,cpu | awk -v p="$pid" -v what="$1" '$1==p {n++; if (n>1) {s+=$2; c++}} END {if (c) printf "idle CPU (%s, 60 s): %.2f%%\n", what, s/c}'
}
cpu "window shown"

stop_app "$pid"
lines=$(wc -l < "$LOG" 2>/dev/null || echo 0)
pid=$(start_app --hidden)
sleep 60
memory "started with --hidden"
cpu "hidden"
stop_app "$pid"
