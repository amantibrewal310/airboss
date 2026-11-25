#!/bin/sh
# Drives the release bundle for end-to-end runs, always on a temporary database.
#
#   tests/e2e/app.sh db DIR                    make DIR with settings (open at login off); prints DIR/airboss.db
#   tests/e2e/app.sh start DB [VAR=value ...] [-- app args]
#                                              launch through LaunchServices with AIRBOSS_DB=DB and
#                                              AIRBOSS_TRACE=1 (plus any VAR=value); prints the pid
#   tests/e2e/app.sh lines DB                  the log lines written since that start
#   tests/e2e/app.sh wait DB PATTERN [SECONDS] wait for such a line (grep -E); prints it
#   tests/e2e/app.sh stop PID
#   tests/e2e/app.sh memory PID                the app plus the WebKit processes it's responsible for
#   tests/e2e/app.sh cpu PID [SECONDS]         average CPU, sampled every 2 s
#
# E2E_APP picks the bundle (default: target/release/bundle/macos/Airboss.app). The real
# database is refused, and each database folder gets an app-settings.json with open at login
# off before the first launch, so no LaunchAgent is registered.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="${E2E_APP:-$ROOT/target/release/bundle/macos/Airboss.app}"
# E2E_LOG: the app's log, when it runs with another HOME.
LOG="${E2E_LOG:-$HOME/Library/Logs/Airboss/app.log}"
STATE="$HOME/Library/Saved Application State/io.github.amantibrewal310.airboss.savedState"
TOOLS="${E2E_TOOLS:-${TMPDIR:-/tmp}/airboss-e2e-tools}"

start_line() { cat "$(dirname "$1")/.log-start" 2>/dev/null || echo 0; }

cmd="${1:-}"; [ $# -gt 0 ] && shift
case "$cmd" in
  db)
    mkdir -p "$1"
    printf '{"hotkey":"Control+Alt+N","open_at_login":false,"appearance":"system","folded":[]}' \
      > "$1/app-settings.json"
    echo "$1/airboss.db"
    ;;
  start)
    db="$1"; shift
    case "$db" in "$HOME/Library/Application Support/Airboss"*) echo "refusing the real database" >&2; exit 1 ;; esac
    [ -f "$(dirname "$db")/app-settings.json" ] || { echo "run: app.sh db $(dirname "$db")" >&2; exit 1; }
    if pgrep -f "Airboss.app/Contents/MacOS/airboss-app( |$)" >/dev/null; then
      echo "another Airboss is running; single-instance would hand over to it" >&2; exit 1
    fi
    (wc -l < "$LOG" 2>/dev/null || echo 0) | tr -d ' ' > "$(dirname "$db")/.log-start"
    # VAR=value pairs go to the app's environment; anything after -- is its arguments.
    envs="--env AIRBOSS_DB=$db --env AIRBOSS_TRACE=1"; args=""; after=0
    for a in "$@"; do
      if [ "$after" = 1 ]; then args="$args $a"; continue; fi
      case "$a" in --) after=1 ;; *) envs="$envs --env $a" ;; esac
    done
    rm -rf "$STATE"
    # shellcheck disable=SC2086
    open -n $envs "$APP" --args $args
    for _ in $(seq 1 100); do
      p=$(pgrep -n -f "$APP/Contents/MacOS/airboss-app" || true)
      [ -n "$p" ] && { echo "$p"; exit 0; }
      sleep 0.05
    done
    echo "the app didn't start" >&2; exit 1
    ;;
  lines)
    tail -n +"$(( $(start_line "$1") + 1 ))" "$LOG"
    ;;
  wait)
    db="$1"; pat="$2"; n=$(( ${3:-10} * 10 ))
    for _ in $(seq 1 "$n"); do
      line=$(tail -n +"$(( $(start_line "$db") + 1 ))" "$LOG" | grep -E "$pat" | tail -1 || true)
      [ -n "$line" ] && { echo "$line"; exit 0; }
      sleep 0.1
    done
    echo "no log line matched: $pat" >&2; exit 1
    ;;
  stop)
    kill -TERM "$1" 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$1" 2>/dev/null || { rm -rf "$STATE"; exit 0; }; sleep 0.3; done
    kill -KILL "$1" 2>/dev/null || true
    rm -rf "$STATE"
    ;;
  memory)
    mkdir -p "$TOOLS"
    if [ ! -x "$TOOLS/resp" ]; then
      cat > "$TOOLS/resp.c" <<'C'
#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
extern pid_t responsibility_get_pid_responsible_for_pid(pid_t);
int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) printf("%s %d\n", argv[i], responsibility_get_pid_responsible_for_pid(atoi(argv[i])));
  return 0;
}
C
      clang -O2 -o "$TOOLS/resp" "$TOOLS/resp.c"
    fi
    total=0
    # shellcheck disable=SC2046
    for p in "$1" $("$TOOLS/resp" $(pgrep -f 'com.apple.WebKit\.' || true) | awk -v app="$1" '$2==app {print $1}'); do
      kb=$(footprint "$p" 2>/dev/null | awk '/Footprint:/ {for (i=1;i<=NF;i++) if ($i=="Footprint:") {v=$(i+1); u=$(i+2)}; if (u=="MB") v*=1024; if (u=="GB") v*=1048576; print int(v); exit}')
      [ -n "$kb" ] || continue
      total=$((total + kb))
      echo "  $(ps -o comm= -p "$p" | xargs basename): $((kb / 1024)) MB"
    done
    echo "memory: $((total / 1024)) MB"
    ;;
  cpu)
    samples=$(( ${2:-30} / 2 + 1 ))
    top -l "$samples" -s 2 -pid "$1" -stats pid,cpu | awk -v p="$1" '$1==p {n++; if (n>1) {s+=$2; c++}} END {if (c) printf "cpu: %.2f%% over %d s\n", s/c, c*2}'
    ;;
  *)
    sed -n '2,16p' "$0"; exit 2
    ;;
esac
