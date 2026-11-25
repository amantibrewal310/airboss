# airboss-services

The contract between the Airboss app and its services (`sync`, `launch`, `attention`): shared types and traits, `std` only. The source of truth is the "Shared contract" section of [SPEC-attention.md](../../SPEC-attention.md#shared-contract). The fields each side reads are in [SPEC-app.md](../../SPEC-app.md) (the `crates/services` section and Appendix A), [SPEC-sync.md](../../SPEC-sync.md) and [SPEC-launch.md](../../SPEC-launch.md).

It depends on `airboss-core` (ids, `Repo`, `PrState`, `Event`, `CoreSnapshot`) and `serde`. It has no tokio and starts no threads. Its only logic is `Changes`, `PrRef`, `ThreadState::chip`, `NeedsKind::group`, the `SyncSnapshot` helpers and `time`.

## Fakes for tests

```toml
[dev-dependencies]
airboss-services = { path = "../services", features = ["fakes"] }
```

`airboss_services::fakes` has `FakeSync`, `FakeLaunch`, `FakeAttention` and `FakeClock`. Each fake records its calls (`calls()`), answers with what the test set (`set_snapshot`, `answer_find_thread`, `answer_lookup`, `set_prepare`, `set_start`, `set_items` …), and bumps its `Changes` when that changes. None of them starts a thread.

## Decisions

Where the specs left a gap, this crate made the smallest choice consistent with all four.

1. **Derives.** Every value type derives `Debug`, `Clone`, `PartialEq` (and `Eq` where it can), `Serialize` and `Deserialize`. The contract names only `Serialize`. `Deserialize` is added because the app reads keys such as `ThreadKey` and `NeedsKey` back from the UI, and so that round trips can be tested. Fieldless enums are `Copy`, as `chip(self)` and `group(self)` need. `ThreadKey`, `Source`, `PrRef`, `NeedsKey` and `NeedsGroup` are also `Hash` and `Ord`, and `NeedsGroup` is declared in display order, so sorting by group gives display order.
2. **`SyncSnapshot`** derives `Default` and `PartialEq` as well as `Clone` and `Debug`: a service starts with an empty one, and tests compare them. It isn't serialisable, because JSON can't key a map by a struct. `StoreChange` stays in the process and isn't serialisable either.
3. **The JSON shape** is serde's default external tagging, in snake_case. Unit variants are strings (`"unknown"`, `"t3_not_running"`). Variants with data are one-key objects (`{"ready":{"version":"…"}}`, `{"no_repo":"/path"}`, `{"pr":{"ref":"acme/web#1","url":null}}`). Every `r#ref` field is `"ref"`. `PrRef` and `NeedsKey` are plain strings. The app maps these types to its own ts-rs DTOs, so this shape stays inside Rust.
4. **`PrRef` copies `core` v3's rule** (`canonical_pr_ref` in `crates/core/src/links.rs`), because `core` keeps its own crate-private. A test runs 34 inputs through `core` and through `PrRef::parse` and checks the two agree. Reading a `PrRef` from JSON goes through `parse`, so `"Acme/Web#42"` is read as `acme/web#42`, and an invalid ref is refused.
5. **Small additions with no logic:** `ServiceError::new(code, message)`, `Display` for `ServiceError` (its message) and `PrRef` (its text), `std::error::Error` for `ServiceError`, and `SystemClock` deriving `Copy` and `Default`.
6. **The helpers' order**, which the contract leaves open:
   - `threads_for`: newest activity first, unknown times last, ties by key.
   - `prs_for`: by owner, then repo, then number as a number (so `#9` comes before `#10`).
   - `untracked`: newest activity first, as the contract says, with ties by key.
   - `my_open_prs`: the PRs in `prs` that are unlinked and `open`, most recently updated first, ties by ref. An unlinked PR in `prs` can only be mine, because `sync` reads no one else's.
   - `suggested_prs(branch)`: those with `head_ref == branch`, in the same order.
   - Times are compared as times (`time::from_ts`), not as text, so a GitHub stamp without milliseconds still sorts correctly. Services should still store every `Timestamp` through `time::to_ts`.
7. **`time`** truncates to the millisecond, towards the past. `from_ts` takes 1 to 9 fraction digits, or none (GitHub's form). It refuses any offset other than `Z`, and impossible dates such as 2025-02-29. Text order is time order for years 0000 to 9999. Checked against macOS `date` and against `core`'s own event times.
8. **Left out on purpose:**
   - `Secret` is `sync`'s own type (SPEC-sync, Interfaces).
   - `Step` is each worker's own. `sync` defines it, and `launch` and `attention` use the same shape in their own crates.
   - Merging connection rows with the same `name` is the app's job. The rule is on `Connection`'s doc comment.
9. **`Changes::bump` saturates** at `u64::MAX` instead of overflowing, so it can never panic while it holds the lock. Nothing gets near that count.
10. **Test-only dependencies:** `serde_json`, already in the workspace lockfile through `mcp`. The crate also depends on itself with the `fakes` feature, so its own tests can use the fakes.
11. **`core` v3's names match the specs.** The names SPEC-attention lists as not verified yet are exactly what `core` v3 built: `CoreSnapshot.edges` as `(waiting, waits_on)`, `ReadyTask.since`, and `TaskNode.subtree_last_event_at`.
