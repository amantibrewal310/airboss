#!/usr/bin/env python3
"""A burst of CLI writes against a running app: how fast catch_up brings them in.

    AIRBOSS_DB=/tmp/x/airboss.db tests/e2e/burst.py --bin path/to/airboss [--n 100] [--parallel 1]

The app must run on the same AIRBOSS_DB with AIRBOSS_TRACE=1. Runs N `airboss add` commands
(PARALLEL at a time), then reads the trace: for each new task, the time from the start of
its command to the first `tree:patch` that created it; how many patches the burst took; and
how long each catch-up batch took inside the app. Exits non-zero if a write fails or a task
never shows.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from live import Log, LOG  # noqa: E402


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p * (len(xs) - 1))))] if xs else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--n", type=int, default=100)
    ap.add_argument("--parallel", type=int, default=1)
    args = ap.parse_args()
    if not os.environ.get("AIRBOSS_DB"):
        sys.exit("set AIRBOSS_DB to the app's temporary database")
    log = Log(LOG)

    def one(i):
        t0 = int(time.time() * 1000)
        out = subprocess.run(
            [args.bin, "add", f"Burst {i}", "--json"], capture_output=True, text=True
        )
        t1 = int(time.time() * 1000)
        if out.returncode != 0:
            return (i, t0, t1, None, out.stderr.strip())
        return (i, t0, t1, json.loads(out.stdout)["id"], None)

    start = int(time.time() * 1000)
    with ThreadPoolExecutor(max_workers=args.parallel) as pool:
        results = list(pool.map(one, range(args.n)))
    end = int(time.time() * 1000)
    errors = [r[4] for r in results if r[4]]
    ids = {r[3]: r for r in results if r[3] is not None}

    # Wait for the patches that create all of them. The trace lists at most 20 ids per
    # patch ("…+N" for the rest), so count instead: ids are given out in commit order and a
    # catch-up takes everything committed so far, so patch k created the next `count_k` ids.
    deadline = time.time() + 10
    shown = {}
    batches = []
    order = sorted(ids)
    while time.time() < deadline:
        log.read()
        shown.clear()
        batches.clear()
        next_id = 0
        for ms, what in log.lines:
            if ms < start:
                continue
            if what.startswith("tree:patch"):
                m = re.search(r"created=\[([^\]]*)\]", what)
                parts = m.group(1).split(",") if m and m.group(1) else []
                count = sum(1 for c in parts if c.isdigit())
                count += sum(int(c[2:]) for c in parts if c.startswith("…+"))
                for i in order[next_id : next_id + count]:
                    shown[i] = ms
                next_id += count
            elif what.startswith("catch_up"):
                took = int(re.search(r"took_us=(\d+)", what).group(1))
                batches.append((ms, took, what))
        if len(shown) == len(ids):
            break
        time.sleep(0.05)

    patches = sum(1 for ms, w in log.lines if w.startswith("tree:patch") and ms >= start)
    lat = [shown[i] - ids[i][1] for i in shown]
    cli = [r[2] - r[1] for r in results]
    core = [t for _, t, w in batches if "core" in w]
    summary = {
        "writes": args.n,
        "parallel": args.parallel,
        "failed": len(errors),
        "errors": errors[:3],
        "burst_ms": end - start,
        "cli_ms": {"median": pct(cli, 0.5), "p95": pct(cli, 0.95), "max": max(cli)},
        "shown": len(shown),
        "write_to_patch_ms": {"median": pct(lat, 0.5), "p95": pct(lat, 0.95), "max": max(lat) if lat else None},
        "last_shown_after_burst_ms": (max(shown.values()) - end) if shown else None,
        "patches": patches,
        "catch_up_batches": len(batches),
        "catch_up_us": {"median": pct(core, 0.5), "p95": pct(core, 0.95), "max": max(core) if core else None},
    }
    print(json.dumps(summary, indent=2))
    if errors or len(shown) != len(ids):
        sys.exit(1)


if __name__ == "__main__":
    main()
