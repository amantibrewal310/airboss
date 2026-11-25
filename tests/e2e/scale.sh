#!/bin/sh
# Performance at scale: boot on a seeded database (2,000 tasks, 1,000 dependencies), then
# cold start (median of RUNS launches to ui_ready), idle memory and CPU, and a burst of CLI
# writes through catch_up.
#
#   tests/e2e/scale.sh WORKDIR [Airboss.app]
#
# WORKDIR gets the database (seeded once with the `seed` example) and nothing else.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
W="${1:?usage: scale.sh WORKDIR [Airboss.app]}"
export E2E_APP="${2:-${E2E_APP:-$ROOT/target/release/bundle/macos/Airboss.app}}"
RUNS="${RUNS:-10}"
APPSH="$ROOT/tests/e2e/app.sh"
AB="$E2E_APP/Contents/MacOS/airboss"

DB="$(sh "$APPSH" db "$W")"
if [ ! -f "$DB" ]; then
  (cd "$ROOT" && cargo run -q -p airboss-app --example seed -- --tasks 2000 --deps 1000 --db "$DB")
fi
now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }

echo "machine: $(sysctl -n machdep.cpu.brand_string), macOS $(sw_vers -productVersion); load $(sysctl -n vm.loadavg)"
echo "tasks: $(sqlite3 "$DB" 'select count(*) from tasks'), dependencies: $(sqlite3 "$DB" 'select count(*) from task_deps')"

times=""
i=0
while [ "$i" -lt "$RUNS" ]; do
  t0=$(now_ms)
  pid=$(sh "$APPSH" start "$DB")
  line=$(sh "$APPSH" wait "$DB" 'ui_ready after [0-9]+ ms; epoch_ms=' 10)
  ready=$(echo "$line" | sed 's/.*epoch_ms=//')
  times="$times $((ready - t0))"
  sh "$APPSH" stop "$pid"
  sleep 1
  i=$((i + 1))
done
echo "cold start, launch to ui_ready (ms):$times"
echo "$times" | tr ' ' '\n' | grep -v '^$' | sort -n | awk '{a[NR]=$1} END {printf "cold start median: %d ms (min %d, max %d)\n", (NR%2 ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2), a[1], a[NR]}'

pid=$(sh "$APPSH" start "$DB")
sh "$APPSH" wait "$DB" 'ui_ready' 10 >/dev/null
sleep 30
echo "idle, 30 s after start:"
sh "$APPSH" memory "$pid"
sh "$APPSH" cpu "$pid" 60

echo "burst: 100 sequential CLI writes"
AIRBOSS_DB="$DB" python3 "$ROOT/tests/e2e/burst.py" --bin "$AB" --n 100 --parallel 1
echo "burst: 100 CLI writes, 8 at a time"
AIRBOSS_DB="$DB" python3 "$ROOT/tests/e2e/burst.py" --bin "$AB" --n 100 --parallel 8
sh "$APPSH" memory "$pid"
sh "$APPSH" stop "$pid"
