#!/bin/sh
# One controlled real Do it (integration QA step 4). Creates a REAL T3 project and thread, so
# run it by hand, once:
#
#   tests/e2e/real_launch.sh WORKDIR
#
# WORKDIR (new, under a temp folder) gets: a bare "origin", a throwaway repo with one commit
# pushed to it, and a temporary Airboss database with the repo registered and one task linked
# to it (through the airboss CLI). Then the ignored test `real_launch` runs the app's own
# do_it with the real tools, adding the repo to T3 first if T3 says it isn't a project.
# The environment is cleaned the way Finder would start the app: no T3 or Claude thread
# variables, so nothing is attributed to the shell's own thread.
set -eu

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
W="${1:?usage: real_launch.sh WORKDIR}"
[ -e "$W" ] && { echo "$W exists; give a new folder" >&2; exit 1; }
mkdir -p "$W"
W="$(cd "$W" && pwd -P)"

git init -q --bare -b main "$W/origin.git"
git init -q -b main "$W/airboss-qa-throwaway"
cd "$W/airboss-qa-throwaway"
printf '# Airboss QA throwaway\n\nMade by Airboss integration QA. Safe to delete.\n' > README.md
git add README.md
git -c user.name="Airboss QA" -c user.email="qa@example.invalid" commit -q -m "First commit"
git remote add origin "$W/origin.git"
git push -q -u origin main
git remote set-head origin main
cd "$ROOT"

(cd "$ROOT" && cargo build -q --release -p airboss-mcp)
AB="$ROOT/target/release/airboss"
DB="$(sh "$ROOT/tests/e2e/app.sh" db "$W/db")"
export AIRBOSS_DB="$DB"
"$AB" repo add airboss-qa-throwaway "$W/airboss-qa-throwaway" --json
TASK=$("$AB" add "QA throwaway: add one line to the README" --repo airboss-qa-throwaway \
  --body "Integration QA for Airboss. In this throwaway repo, add the line 'Hello from Airboss QA.' to README.md and commit it on your branch. The repo has no GitHub remote, so there's no PR to open: commit, say so, and stop. Don't install anything." \
  --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "task $TASK in $DB"

env -u T3CODE_THREAD_ID -u CLAUDE_CODE_SESSION_ID -u AIRBOSS_THREAD -u T3CODE_DESKTOP_HOST_SCOPE \
  -u T3CODE_CLI_PATH -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT \
  AIRBOSS_DB="$DB" AIRBOSS_QA_TASK="$TASK" \
  cargo test -q -p airboss-app --test real_launch -- --ignored --nocapture 2>&1 | tee "$W/real_launch.log"
echo "log: $W/real_launch.log"
"$AB" show "$TASK"
