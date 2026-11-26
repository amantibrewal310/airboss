# Contributing

Every change to Airboss comes with tests, and every check below passes before it merges.

Read [docs/architecture.md](docs/architecture.md) first for the map, then the spec of the module you're changing. The specs are listed in [CAPABILITIES.md](CAPABILITIES.md).

## Set up

You need a Mac with Apple silicon, [rustup](https://rustup.rs), Node 24 and pnpm. The repo pins Rust 1.100 with clippy and rustfmt in `rust-toolchain.toml`, so rustup installs it on the first build.

```sh
git clone https://github.com/amantibrewal310/airboss.git
cd airboss
cargo build --workspace
cd app
pnpm install
pnpm exec playwright install webkit     # once, for the component and end-to-end tests
```

## Run it

```sh
cd app
pnpm dev:mock          # the interface in a browser, on the mock backend: http://localhost:5173
pnpm tauri dev         # the real app, built in debug
```

- **The mock** needs no Rust. It picks its data with `?fixture=`: `full` (the default), `design`, `perf` (2,000 tasks), `empty`, `welcome`, `t3missing`, `nothing` and `dberror`. [app/README.md](app/README.md) has the rest.
- **The real app** uses your real database unless you set `AIRBOSS_DB`. Use a throwaway one: `AIRBOSS_DB=/tmp/ab.db pnpm tauri dev`. A development build never turns on open at login.
- **A big database** for trying things or measuring:

  ```sh
  cargo run -p airboss-app --example seed -- --tasks 2000 --db /tmp/ab.db
  ```

  `--design` seeds the design's sample data instead. The seeder refuses to run without `--db`, so it never touches your real database.

- **The CLI** against the same throwaway file: `AIRBOSS_DB=/tmp/ab.db cargo run -p airboss-mcp -- ls`.

## Check it

Run all of these before every commit. They're what "done" means here.

```sh
# Rust, from the repo root
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check

# The interface, from app/
pnpm check             # svelte-check and tsc; warnings fail
pnpm test              # Vitest: unit tests in Node, component tests in WebKit
pnpm test:e2e          # Playwright on the mock build: smoke test, 60 fps check, accessibility, screenshots
```

Some tests are `#[ignore]` because they're slow or read real files. Run them when you touch that area:

```sh
cargo test -p airboss-core --release -- --ignored perf                # performance checks
cargo test -p airboss-mcp --release -- --ignored perf                 # start-up and call times
cargo test -p airboss-app --release --test perf -- --ignored          # rebuild, diff and write timings
cargo test -p airboss-sync --release --test real_t3 -- --ignored      # T3 Code's real database, read-only
cargo test -p airboss-sync --test real_herdr -- --ignored             # the real hc: a probe terminal it starts and removes
cargo test -p airboss-launch --test real_herdr -- --ignored           # Herdr open: tabs in its own airboss-test space, then closed
```

**Generated files.** The TypeScript types in `app/src/lib/types/generated/` and the mock's fixtures are written from Rust. After changing a DTO or a recorded command, regenerate them and commit the result:

```sh
cargo test -p airboss-app --test fixtures
AIRBOSS_FIXTURES_CHECK=1 cargo test -p airboss-app --test fixtures    # fails if anything is out of date
```

**Screenshots.** `pnpm exec playwright test --update-snapshots` rewrites the baselines in `app/tests/e2e/baselines/`. Compare each changed one with `docs/design/screens/` before you commit it.

**By hand.** What the automated tests can't reach (the real web view, native menus, the hotkey, the menu-bar menu, VoiceOver) is a checklist in [app/QA.md](app/QA.md). Run it in a release build before merging to main.

**Performance.** `app/src-tauri/scripts/measure.sh` measures a release build against the targets in [CAPABILITIES.md](CAPABILITIES.md): download size, cold start, the hotkey, idle memory and CPU. It uses a throwaway database.

## How work is done

Every module follows the same lifecycle, from the agent-skills set:

1. **Spec.** Write or update `SPEC-<module>.md`: the objective, decisions, interfaces, behaviour, errors, performance targets, tests and boundaries. Anything not yet checked goes under "Not verified yet".
2. **Fresh-eyes review** of the spec, by someone who hasn't seen it.
3. **Plan.** Break it into small tasks, in `tasks/<module>-plan.md` and `tasks/<module>-todo.md`.
4. **Build, tests first.** Write the failing test, make it pass, then tidy. One thin slice at a time.
5. **Review, then simplify.** A code review on correctness, readability, architecture, security and performance, then a pass to make the code plainer.
6. **Merge.**

When a build differs from its spec, the spec gets a "Decisions made while building" entry, so the spec stays true. Program-wide decisions are recorded in [tasks/program.md](tasks/program.md).

A small fix doesn't need a new spec, but it still starts with a failing test.

## Rules of the code

- **Rust:** rustfmt, clippy with `-D warnings`, and no `unwrap` or `expect` outside tests. Wrapper types for ids, and a short "why" doc comment on public items.
- **Unsafe code** only in `app/src-tauri/src/macos.rs`, with a `SAFETY:` comment on each block. The rest of the workspace forbids it.
- **SQL** only in `core`, and in each service crate for its own tables. Everything else goes through `Store`. Migrations are append-only, numbered files.
- **No polling.** Threads block on channels, condvars or kqueue. The one timer is the GitHub pass.
- **No network** from the app, and no port anywhere, with one exception: the updater (`updates.rs`) reads `updates/latest.json` from GitHub and downloads a release, verifying its signature ([SPEC-updates](SPEC-updates.md)). External tools are `gh`, `t3` and `git`, each called from one module.
- **The interface:** TypeScript in strict mode, Svelte 5 runes only, styles scoped per component, colours and sizes from `app/src/styles/tokens.css`. Every shortcut goes in the command registry, `app/src/lib/logic/commands.ts`.
- **Dependencies:** ask before adding a crate or an npm package. Versions are pinned exactly.
- **Design:** [DESIGN.md](DESIGN.md) is the rule for every pixel. Red means "needs you" and nothing else.
- **No company names** in the product or its docs, apart from the tools Airboss works with.

## Commits

- Plain messages: a short summary line saying what the change does, such as "Watch T3's WAL with kqueue, and re-arm it". Add a body only when the why isn't obvious.
- One logical change per commit, with its tests.
- **No AI attribution.** No "Co-Authored-By" lines for tools or assistants, and no "Generated with" footers, in commits or pull requests.
- Work on a branch, and merge to `main` only when every check passes.

## Releasing

Versions follow [Semantic Versioning](https://semver.org). Alphas are `0.1.0-alpha.N`, with macOS build number `N`; the build number only ever goes up.

1. Set the version:
   - `version` under `[workspace.package]` in the root `Cargo.toml`. Every crate inherits it, so `airboss --version`, the MCP server and Help › Report a Problem… report it.
   - `version` in `app/src-tauri/tauri.conf.json` and `app/package.json`, the same string.
   - `bundle.macOS.bundleVersion` in `tauri.conf.json`: the build number.
   - `CFBundleShortVersionString` in `app/src-tauri/Info.plist`, only when the three numbers change. Apple allows only three numbers there, so it's `0.1.0` for every `0.1.0-alpha.N`. About Airboss shows the full version.
2. Move [Unreleased] in `CHANGELOG.md` to a section for the new version, dated, and add its compare link at the bottom.
3. Run every check above. `cargo test -p airboss-app --test version` fails if any of step 1 or 2 disagrees.
4. Build with `sh app/src-tauri/scripts/release-build.sh`. It needs the updater's private key, `~/.tauri/airboss-updater.key`, and its password in the login keychain item `airboss-updater-key` ([SPEC-updates](SPEC-updates.md)); neither is in the repo. It builds the `.dmg` and the signed update archive, checks the app's signature, its Info.plist versions and the bundled CLI's version, and leaves the four release files in `target/release/bundle/release/`.
5. Tag `vX.Y.Z-alpha.N` (annotated: `git tag -a`), push the tag, and publish a pre-release with those four files: `gh release create vX.Y.Z-alpha.N --prerelease --notes-file … target/release/bundle/release/*`. The notes start from the changelog's section.
6. Only then, publish the update: `node app/src-tauri/scripts/update-manifest.mjs X.Y.Z-alpha.N` writes `updates/latest.json` (it refuses until the release's archive downloads), and committing it to `main` is what tells installed apps. To hold an update back, don't commit it.

## Reporting a problem

**Help › Report a Problem…** opens the bug form with your versions filled in. Say what you did, what you expected and what happened. Include the relevant lines of `~/Library/Logs/Airboss/app.log`, and for a connection problem, what its row in the connections popover says. [docs/troubleshooting.md](docs/troubleshooting.md) covers the common ones.
