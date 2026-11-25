# Airboss app

The Mac app's user interface: Svelte 5 (runes only) on Vite, in a Tauri 2 shell. The Rust side
lives in `src-tauri/` and is built separately. The contract between them is
[SPEC-app.md](../SPEC-app.md), Appendix A (commands) and B (events); the look is
[DESIGN.md](../DESIGN.md).

## Run it

Node 24 and pnpm.

```
pnpm install
pnpm exec playwright install webkit     # once, for component and end-to-end tests
pnpm dev:mock                           # the UI in a browser on the mock backend, http://localhost:5173
pnpm tauri dev                          # the real app (needs src-tauri)
```

The mock picks its data with `?fixture=`: `full` (default, about 60 tasks), `design` (the
design's sample data), `perf` (2,000 tasks), `empty` and `welcome` (first run), `t3missing`,
`nothing` and `dberror`. `?view=all&select=144` starts somewhere other than Needs me. The quick-add
panel is at `/quickadd.html`.

## Check it

```
pnpm check        # svelte-check and tsc, warnings fail
pnpm test         # Vitest: unit tests in Node, component tests in WebKit
pnpm test:e2e     # Playwright on the mock build: smoke test, 60 fps check, accessibility, 24 screenshot baselines
pnpm build        # dist/ for Tauri (index.html and quickadd.html)
```

`pnpm exec playwright test --update-snapshots` rewrites the baselines in `tests/e2e/baselines/`.
Compare any change with `docs/design/screens/` before you commit new ones.

## Layout

| Path | What |
|---|---|
| `src/lib/types/` | The DTOs, hand-written and **provisional** until ts-rs generates them. Everything imports them through `index.ts`. |
| `src/lib/api/` | `contract.ts` (every command and event with its types), `index.ts` (one function per command), `transport.ts`, `tauri.ts` |
| `src/lib/mock/` | The browser's backend: model, row builder, write handlers, undo, attention stand-in, and `replay.ts` for recorded scripts. `fixtures/index.ts` is the one loader, where the Rust-generated fixtures plug in. |
| `src/lib/state/` | Stores (`*.svelte.ts`) and the controller (`app.svelte.ts`) that holds every action |
| `src/lib/logic/` | Pure, tested logic: views, selection, virtual-list maths, patches, keymap, focus zones, the command registry, search, run-map layout, links, time, connections, launch toasts |
| `src/lib/components/` | Small parts: StatusIcon, ProgressRing, StackedBar, LiveChip, Avatar, Kbd, Button, Menu, Popover, Modal, VirtualList… |
| `src/windows/main/` | The main window: sidebar, list, panel, page, run map, Needs me, Plans, overlays, first run |
| `src/windows/quickadd/` | The quick-add panel (its own entry page) |
| `tests/unit`, `tests/component`, `tests/e2e` | Vitest (Node), Vitest (WebKit), Playwright (WebKit) |

No component library, no CSS framework, no state or virtual-list library: plain CSS on the
tokens in `src/styles/tokens.css`.
