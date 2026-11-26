#!/bin/sh
# Fails if a commit message credits an AI tool or assistant.
#
#   scripts/check-no-ai-attribution.sh             every commit reachable from HEAD
#   scripts/check-no-ai-attribution.sh main..HEAD  the commits in a range
#   scripts/check-no-ai-attribution.sh --file F    one message file (a commit-msg hook passes it)
#
# It looks for the two things CONTRIBUTING.md rules out: a Co-Authored-By trailer that
# names an assistant, and a "Generated with/by <assistant>" footer. Same input, same answer.
set -eu

TOOLS='claude|anthropic|copilot|openai|chatgpt|codex|gemini|cursor|aider|devin|\bai\b'
PATTERN="^[[:space:]]*(co-authored-by:.*($TOOLS)|generated (with|by)\\b.*($TOOLS))"

scan() { LC_ALL=C grep -inE "$PATTERN" || true; }

if [ "${1:-}" = "--file" ]; then
	hits=$(scan <"${2:?usage: $0 --file MESSAGE_FILE}")
	if [ -n "$hits" ]; then
		echo "AI attribution in the commit message:" >&2
		echo "$hits" >&2
		exit 1
	fi
	exit 0
fi

range=${1:-HEAD}
bad=0
for sha in $(git rev-list "$range"); do
	hits=$(git show -s --format=%B "$sha" | scan)
	if [ -n "$hits" ]; then
		echo "AI attribution in $(git show -s --format='%h %s' "$sha"):" >&2
		echo "$hits" | sed 's/^/    /' >&2
		bad=1
	fi
done
if [ "$bad" -ne 0 ]; then
	echo "Remove the attribution lines (see CONTRIBUTING.md, Commits)." >&2
	exit 1
fi
echo "No AI attribution in $(git rev-list --count "$range") commits."
