#!/usr/bin/env python3
"""Live updates across processes: CLI and MCP writes reaching a running app.

    AIRBOSS_DB=/tmp/x/airboss.db tests/e2e/live.py --bin path/to/airboss [--pr owner/repo#n]

The app must already run on the same AIRBOSS_DB with AIRBOSS_TRACE=1, so it logs each
`tree:patch`, `needs:changed`, menu-bar redraw and catch-up batch with epoch_ms. Each step
makes one write in another process and waits for the trace line that shows it, then reports
how long that took (from the moment the write started) and the patch's seq. `--pr` names a
merged PR the logged-in `gh` can read: linking it must let sync finish the task by itself.
Exits non-zero if a step's patch doesn't arrive or seq ever goes backwards.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mcp_client import Mcp  # noqa: E402

LOG = os.path.expanduser("~/Library/Logs/Airboss/app.log")
TRACE = re.compile(r"trace (?P<what>.*); epoch_ms=(?P<ms>\d+)")


def now_ms():
    return int(time.time() * 1000)


class Log:
    def __init__(self, path):
        self.path = path
        self.offset = os.path.getsize(path)
        self.lines = []  # (epoch_ms, text), everything read since the start

    def read(self):
        with open(self.path, "rb") as f:
            f.seek(self.offset)
            data = f.read()
        if not data.endswith(b"\n"):
            data = data[: data.rfind(b"\n") + 1]
        self.offset += len(data)
        for raw in data.decode("utf-8", "replace").splitlines():
            m = TRACE.search(raw)
            if m:
                self.lines.append((int(m["ms"]), m["what"]))

    def wait(self, pred, since_ms, timeout=5.0):
        """The first trace line at or after since_ms that pred accepts."""
        deadline = time.time() + timeout
        while True:
            self.read()
            for ms, what in self.lines:
                if ms >= since_ms and pred(what):
                    return ms, what
            if time.time() > deadline:
                return None, None
            time.sleep(0.01)


def seq_of(line):
    m = re.search(r"seq=(\d+)", line or "")
    return int(m.group(1)) if m else None


def ids_in(field, line):
    m = re.search(field + r"=\[([^\]]*)\]", line or "")
    return m.group(1).split(",") if m and m.group(1) else []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--pr", help="a merged PR, owner/repo#n")
    args = ap.parse_args()
    if not os.environ.get("AIRBOSS_DB"):
        sys.exit("set AIRBOSS_DB to the app's temporary database")
    log = Log(LOG)
    results = []
    failed = []

    def cli(*argv):
        t0 = now_ms()
        out = subprocess.run([args.bin, *argv, "--json"], capture_output=True, text=True)
        if out.returncode != 0:
            return t0, None, out.stderr.strip()
        return t0, json.loads(out.stdout), None

    def step(name, t0, pred, timeout=5.0, also=None):
        ms, line = log.wait(pred, t0, timeout)
        row = {
            "step": name,
            "latency_ms": (ms - t0) if ms else None,
            "seq": seq_of(line),
            "line": line,
        }
        if also:
            row.update(also)
        if ms is None:
            failed.append(name)
        results.append(row)
        print(json.dumps(row), flush=True)
        return line

    def patch_with(token):
        return lambda w: w.startswith("tree:patch") and token in w

    t0, p, _ = cli("add", "QA live parent")
    step("add parent", t0, patch_with(f"created=[{p['id']}]"))
    P = p["id"]
    t0, a, _ = cli("add", "QA child A", "--under", str(P))
    A = a["id"]
    step("add child A (nest)", t0, patch_with(f"created=[{A}]"))
    t0, b, _ = cli("add", "QA child B", "--under", str(P))
    B = b["id"]
    step("add child B (nest)", t0, patch_with(f"created=[{B}]"))
    t0, _, err = cli("wait", str(B), "--on", str(A))
    step("B waits on A", t0, patch_with(f"{B}:todo:waiting"), also={"error": err})

    if args.pr:
        t0, link, err = cli("link", str(A), "pr", args.pr)
        step("link A to a merged PR", t0, lambda w: w.startswith("tree:patch") or w.startswith("catch_up"), also={"error": err})
        # sync looks the PR up in the background and finishes A itself.
        step("sync finishes A (PR merged)", t0, patch_with(f"{A}:done"), timeout=90)
        step("B became ready (needs me)", t0, lambda w: w.startswith("needs:changed") and f":{B}" in w, timeout=30)
        step("menu bar redrawn", t0, lambda w: w.startswith("tray "), timeout=30)
    else:
        t0, _, err = cli("done", str(A), "--note", "QA")
        step("done A", t0, patch_with(f"{A}:done"), also={"error": err})

    # Proposals come from an agent over MCP; accept and reject are mine.
    thread = f"qa-live-{os.getpid()}"
    m = Mcp(args.bin, os.environ["AIRBOSS_DB"], thread)
    m.initialize()
    t0 = now_ms()
    ok, proposed, _ = m.call(
        "propose_tasks",
        {"parent": P, "tasks": [{"title": "QA proposal 1"}, {"title": "QA proposal 2"}]},
    )
    m.close()
    X, Y = proposed[0]["id"], proposed[1]["id"]
    step("agent proposes 2 (MCP)", t0, patch_with(f"{X}:proposed"))
    step("proposal in needs me", t0, lambda w: w.startswith("needs:changed") and "proposed" in w)
    step("menu bar count", t0, lambda w: w.startswith("tray ") and "proposed" in w)
    t0, _, err = cli("reject", str(X), "--note", "QA: not needed")
    step("reject 1", t0, lambda w: w.startswith("tree:patch") and str(X) in ids_in("removed", w), also={"error": err})
    t0, _, err = cli("accept", str(Y))
    step("accept 1", t0, patch_with(f"{Y}:todo"), also={"error": err})
    step(
        "proposal leaves needs me",
        t0,
        lambda w: w.startswith("needs:changed") and "proposed" not in w,
    )
    t0, _, err = cli("done", str(Y), "--note", "QA")
    step("finish a task (done)", t0, patch_with(f"{Y}:done"), also={"error": err})

    log.read()
    seqs = [seq_of(w) for _, w in log.lines if w.startswith("tree:patch")]
    rising = all(x < y for x, y in zip(seqs, seqs[1:]))
    # Our own writes only: sync's finish waits on GitHub, and needs and the menu bar follow.
    lat = sorted(
        r["latency_ms"]
        for r in results
        if r["latency_ms"] is not None
        and (r["line"] or "").startswith("tree:patch")
        and "sync" not in r["step"]
    )
    summary = {
        "patch_seqs": seqs,
        "seq_strictly_rising": rising,
        "cli_write_to_patch_ms": {"min": lat[0], "median": lat[len(lat) // 2], "max": lat[-1]} if lat else None,
        "failed": failed,
    }
    print(json.dumps(summary), flush=True)
    if failed or not rising:
        sys.exit(1)


if __name__ == "__main__":
    main()
