#!/usr/bin/env python3
"""A scripted MCP client for `airboss mcp`: newline-delimited JSON-RPC over stdio.

    tests/e2e/mcp_client.py --bin path/to/airboss --db /tmp/x/airboss.db \
        --thread qa-fake-1 --task 3 --plan path/to/plan.md

Runs one agent's session against an existing task: initialize, tools/list, list_tasks,
get_task, link (this thread, no ref), update_task to doing then in_review, propose_tasks
under the task, import_plan with a real file, and a few calls that must fail with a readable
error. Prints one JSON object per step on stdout ({"step", "ok", "ms", "result" | "error"})
and exits non-zero if any expectation fails. Never touches the real database: --db is
required, and the identity variables of the calling shell are dropped.
"""

import argparse
import json
import os
import subprocess
import sys
import time

DROPPED = ("T3CODE_THREAD_ID", "CLAUDE_CODE_SESSION_ID", "AIRBOSS_THREAD")


class Mcp:
    def __init__(self, binary, db, thread=None, cwd=None):
        env = {k: v for k, v in os.environ.items() if k not in DROPPED}
        env["AIRBOSS_DB"] = db
        if thread:
            env["T3CODE_THREAD_ID"] = thread
        self.proc = subprocess.Popen(
            [binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            cwd=cwd,
            text=True,
            bufsize=1,
        )
        self.next_id = 1

    def _send(self, msg):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def request(self, method, params=None):
        rid = self.next_id
        self.next_id += 1
        msg = {"jsonrpc": "2.0", "id": rid, "method": method}
        if params is not None:
            msg["params"] = params
        t0 = time.perf_counter()
        self._send(msg)
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("server closed stdout: " + self.proc.stderr.read())
            reply = json.loads(line)
            if reply.get("id") == rid:
                return reply, (time.perf_counter() - t0) * 1000

    def notify(self, method, params=None):
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        self._send(msg)

    def initialize(self):
        reply, ms = self.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "airboss-e2e", "version": "1"},
            },
        )
        self.notify("notifications/initialized")
        return reply, ms

    def call(self, tool, args):
        """(ok, value, ms): value is the parsed JSON result, or the error text."""
        reply, ms = self.request("tools/call", {"name": tool, "arguments": args})
        if "error" in reply:
            return False, reply["error"].get("message", ""), ms
        result = reply["result"]
        text = "".join(c.get("text", "") for c in result.get("content", []))
        if result.get("isError"):
            return False, text, ms
        try:
            return True, json.loads(text), ms
        except ValueError:
            return True, text, ms

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=5)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--db", required=True)
    ap.add_argument("--thread", required=True, help="a fake T3 thread id")
    ap.add_argument("--task", type=int, required=True, help="an open task with no PR")
    ap.add_argument("--plan", required=True, help="a plan file that exists")
    args = ap.parse_args()

    failures = []

    def out(step, ok, ms, value, expect_ok=True, check=None):
        good = ok == expect_ok and (check is None or check(value))
        if not good:
            failures.append(step)
        print(
            json.dumps(
                {
                    "step": step,
                    "ok": ok,
                    "as_expected": good,
                    "ms": round(ms, 2),
                    ("result" if ok else "error"): value,
                }
            ),
            flush=True,
        )
        return value

    m = Mcp(args.bin, args.db, args.thread)
    actor = f"agent:t3/{args.thread}"
    reply, ms = m.initialize()
    info = reply.get("result", {})
    out(
        "initialize",
        "result" in reply,
        ms,
        {
            "serverInfo": info.get("serverInfo"),
            "instructions_name_actor": actor in info.get("instructions", ""),
        },
        check=lambda v: v["instructions_name_actor"],
    )

    reply, ms = m.request("tools/list")
    names = sorted(t["name"] for t in reply["result"]["tools"])
    out(
        "tools/list",
        True,
        ms,
        names,
        check=lambda v: v
        == ["get_task", "import_plan", "link", "list_tasks", "propose_tasks", "update_task"],
    )

    ok, v, ms = m.call("list_tasks", {})
    out("list_tasks", ok, ms, {"total": v.get("total")} if ok else v)

    ok, v, ms = m.call("get_task", {"id": args.task})
    out(
        "get_task",
        ok,
        ms,
        {"id": v["task"]["id"], "status": v["task"]["status"], "links": len(v["links"])}
        if ok
        else v,
        check=lambda v: v["id"] == args.task,
    )

    ok, v, ms = m.call("link", {"id": args.task, "kind": "t3_thread"})
    out(
        "link (no ref)",
        ok,
        ms,
        v,
        check=lambda v: v.get("ref") == args.thread and v.get("linked_by") == actor,
    )

    ok, v, ms = m.call("list_tasks", {"mine": True})
    out(
        "list_tasks mine",
        ok,
        ms,
        [t["id"] for t in v.get("tasks", [])] if ok else v,
        check=lambda v: v == [args.task],
    )

    for status, note in (("doing", None), ("in_review", "Opened the PR; ready for review.")):
        a = {"id": args.task, "status": status}
        if note:
            a["note"] = note
        ok, v, ms = m.call("update_task", a)
        out(f"update_task {status}", ok, ms, v, check=lambda v, s=status: v.get("status") == s)

    ok, v, ms = m.call(
        "propose_tasks",
        {
            "parent": args.task,
            "tasks": [
                {"title": "QA proposal to accept", "body": "Accept me."},
                {"title": "QA proposal to reject"},
            ],
        },
    )
    out(
        "propose_tasks",
        ok,
        ms,
        [(t["id"], t["status"]) for t in v] if ok else v,
        check=lambda v: [s for _, s in v] == ["proposed", "proposed"],
    )

    plan = [
        {
            "key": "a",
            "title": "QA plan: build the thing",
            "children": [
                {"key": "a1", "title": "QA plan: part one"},
                {"key": "a2", "title": "QA plan: part two", "depends_on": ["a1"]},
            ],
        },
        {"key": "b", "title": "QA plan: ship it", "depends_on": ["a"]},
    ]
    ok, v, ms = m.call("import_plan", {"plan_file": args.plan, "plan": plan})
    out("import_plan", ok, ms, v)
    ok, v2, ms = m.call("import_plan", {"plan_file": args.plan, "plan": plan})
    out("import_plan again (idempotent)", ok, ms, v2)

    # Errors an agent will hit; each must say what to do.
    ok, v, ms = m.call("get_task", {"id": 999999})
    out("error: no such task", ok, ms, v, expect_ok=False)
    ok, v, ms = m.call("update_task", {"id": args.task, "status": "todo"})
    out("error: a status agents can't set", ok, ms, v, expect_ok=False)
    ok, v, ms = m.call("link", {"id": args.task, "kind": "slack"})
    out("error: link with no ref for slack", ok, ms, v, expect_ok=False)
    ok, v, ms = m.call("import_plan", {"plan_file": "/nonexistent/plan.md", "plan": plan})
    out("error: plan file missing", ok, ms, v, expect_ok=False)
    ok, v, ms = m.call("list_tasks", {"bogus": 1})
    out("error: unknown argument", ok, ms, v, expect_ok=False)

    m.close()
    if failures:
        print(json.dumps({"failed": failures}), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
