# Contributing

Thanks for looking. Kelyfos is a small, opinionated app, so the most useful
thing before writing code is a quick issue describing what you want to change —
it is cheaper to disagree about scope in a paragraph than in a diff.

## Setup

Node 20+, Rust (stable), and on Windows the MSVC build tools and the WebView2
runtime.

```sh
npm install
npm run tauri dev              # run the app
npm run build                  # frontend typecheck (tsc && vite build)
cd src-tauri && cargo test     # Rust tests
```

There is no frontend test runner and no ESLint config. `npm run build` is the
typecheck, and it must pass. Rust tests live in `#[cfg(test)]` modules inside
the file they cover.

## Where things are

`CLAUDE.md` is the architecture document — the IPC seam, storage and
migrations, terminal status resolution, the process layer, and the frontend
structure. Read the part covering whatever you are touching. `docs/PLAN.md` is
the original design doc and is partly stale; read it for intent, not as a map.

Adding a backend command means touching four places, and missing one fails at
runtime rather than at compile time:

1. the `#[tauri::command]` function
2. the `generate_handler![...]` list in `src-tauri/src/lib.rs`
3. `src/lib/ipc.ts` — the single typed wrapper over every `invoke`
4. the matching type in `src/lib/types.ts`

Components never call `invoke` directly; they go through a store, and stores go
through `ipc`.

## Things that will get a patch sent back

These are not style preferences. Each one is a bug the project has already had.

- **Authentication is OpenSSH's.** Kelyfos builds an `ssh` command line and
  types it into a shell; it does not implement any part of connecting. Before
  adding anything to that path, answer "why can't `ssh` do this?"
- **A shipped migration is immutable.** Fix one with a new step, never by
  editing it. A new column belongs in both `SCHEMA` and a migration step — miss
  the step and `row_to_*` fails on `row.get("col")?`, which empties the whole
  sidebar rather than blanking one field.
- **Terminal status precedence lives only in `src/lib/agentStatus.ts`.** Do not
  re-derive it in a component. Its inputs are deliberately separate maps; fold
  them into one field and whichever event lands last wins.
- **Anything Claude-related must degrade silently.** A machine that has never
  heard of Claude Code must behave identically.
- **Comments here explain *why*, at length, and are load-bearing.** Several
  document decisions that were reverted once already — title-matching for
  status, output-matching for errors, `.ok()` on migration failures. Read them
  before changing the behaviour they guard, and keep the density when adding
  code.
- **Errors that reach the user are written for the user**, not a debug dump.

## Platform gotchas

The app targets macOS and Windows, and `cargo check` on one tells you nothing
about the other. Code behind `#[cfg(windows)]` is invisible to a macOS build,
including the imports it needs — an import used only by Windows code must carry
the same `cfg`, or macOS warns it is unused and the obvious "fix" breaks the
Windows build. This has happened twice.

Keep the busy classifier in `modules/process` free of `cfg`: that is how macOS
tests cover the Windows logic.

CI builds macOS (arm64 + x86_64) and Windows on every tag, so a platform break
surfaces there — but not before your PR merges, so try to reason about both.

## Pull requests

- One concern per PR.
- Say what you changed and why the alternative was worse. If a comment in the
  code already answers that, point at it.
- `npm run build` and `cargo test` both pass.
- New behaviour that can break silently gets a test. Status resolution,
  migrations, and the SSH command builder all have existing tests to copy from.

## Forking

If you fork this and cut your own releases, change `identifier` and
`plugins.updater.endpoints` in `src-tauri/tauri.conf.json` and generate your own
signing key (`npx tauri signer generate -w ~/.tauri/yourapp.key`), then put the
public key in `plugins.updater.pubkey`. Otherwise your builds check this repo
for updates and reject them anyway, since you will not have the private key
that matches the baked-in public one.
