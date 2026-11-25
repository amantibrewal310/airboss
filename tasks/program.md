# Program: Airboss v1

Who drives it: Claude, acting as engineering manager, with a team of up to five agents. The user delegated the whole build on 25 Nov 2026: "drive it end to end, testing and bug fixes included; high quality, polished, great UX". The user comes back in only for decisions that can't be made without them.

Inputs: [docs/idea.md](../docs/idea.md), [CAPABILITIES.md](../CAPABILITIES.md), [PRODUCT.md](../PRODUCT.md), [DESIGN.md](../DESIGN.md) (with the prototype in `docs/design/`), and one spec per module.

## Process (agent-skills lifecycle, per module)

1. Spec, saved as `SPEC-<module>.md`.
2. Fresh-eyes review of the spec.
3. Task plan.
4. Build, tests first.
5. Code review, then simplification.
6. Merge.

Then, for the whole product:

7. End-to-end QA as a real user.
8. Polish passes until the bar in "Done means" is met.

## Waves

| Wave | Work | State |
|---|---|---|
| 0 | `core` v1 | Done (PR #1) |
| 0 | App design | Done (merged e63a45b) |
| 1 | `core` v2: what the design needs (see SPEC-core "v2") | Next |
| 1 | Specs: `app`, `sync`, `launch`, `attention` | Next |
| 1→2 | `mcp`: spec drafted; build after `core` v2 lands | Next |
| 2 | Build `app`, `sync`, `launch`, `attention` in parallel | |
| 3 | Integration: one app process hosts sync, launch and attention; the CLI and `airboss mcp` share the database | |
| 3 | QA: real-user flows, performance targets, light and dark, keyboard-only | |
| 4 | Polish and bug-fix iterations; README and install instructions | |

## Decisions made as EM (the user may revisit them)

- **Several repos per task**, per the design (agreed with the user in the design thread). This replaces the single `tasks.repo_id`. Do it starts one thread per linked repo, on one branch name shared by all of them.
- **Migrations:** `001_core.sql` may still be edited, because no real database exists yet. Once `mcp` ships, migrations are append-only.
- **`mcp` connects over stdio, one process per thread** (SPEC-mcp). The app watches the database file to pick up writes from other processes.
- **Sync, launch and attention run inside the app process.** The CLI gets `--once` commands where that's useful.

## Done means

- Every module's success criteria are checked off, with tests.
- Every performance target in CAPABILITIES is measured on this Mac.
- The app is used end to end, keyboard-only, in dark and light: capture a task, plan it, Do it, an agent reports, a PR merges, Needs me clears.
- `cargo clippy -D warnings`, `cargo fmt --check`, every Rust test and every UI test are clean.
- No AI attribution anywhere.
