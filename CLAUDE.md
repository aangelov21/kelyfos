# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Kelyfos — a local-first SSH/terminal workspace manager. Tauri 2 (Rust) shell around a
React 19 + TypeScript + Zustand frontend, xterm.js terminals, FlexLayout docking,
SQLite storage. No server, no accounts, no telemetry.

## Commands

```sh
npm install
npm run tauri dev                # run the app (starts Vite on :1420, then the Rust host)
npm run tauri build              # installers + updater artifacts
npm run build                    # frontend only: tsc && vite build — this is the typecheck
npx tsc --noEmit                 # typecheck without emitting

cd src-tauri && cargo test                     # all Rust tests
cd src-tauri && cargo test migrations          # one module's tests
cd src-tauri && cargo test current_database_runs_nothing   # one test by name
cd src-tauri && cargo check                    # fast compile check
```

There is no frontend test runner and no ESLint config. Rust tests are `#[cfg(test)]`
modules inside the file they cover (`migrations.rs`, `marks.rs`, `pty/mod.rs`,
`agent/*.rs`, `backup/mod.rs`, `fonts/mod.rs`, `shell_integration/mod.rs`).

## Architecture

### The IPC seam

`src/lib/ipc.ts` is the single typed wrapper over every `invoke`. Its methods map
1:1 to the `generate_handler![...]` list in `src-tauri/src/lib.rs`. Adding a
backend command means touching four places: the `#[tauri::command]` fn, the
handler list, `ipc.ts`, and the matching type in `src/lib/types.ts`.

Rust structs crossing the boundary use `#[serde(rename_all = "camelCase")]`, so
`src/lib/types.ts` mirrors `src-tauri/src/modules/models.rs` field for field.
Command *arguments* are camelCase on the JS side and snake_case in Rust — Tauri
converts them.

Rust → JS is events, not returns:

- `pty:data:{terminalId}` — base64 bytes, written straight into xterm
- `pty:status:{terminalId}` — `{ status, message }`
- `agent:state` — one `AgentSnapshot` per change

New frontend calls to Tauri *plugins* (fs, dialog, updater, process) may also
need a permission in `src-tauri/capabilities/default.json`.

### Terminals are local PTYs, not SSH sessions

`modules/pty` spawns a real shell on a PTY per terminal. A saved connection does
not get its own transport — an `ssh …` line built by `build_ssh_command` is typed
into the shell, and **authentication is entirely OpenSSH's** (`~/.ssh/config`,
ssh-agent, known_hosts). The app implements no part of connecting, so before
adding anything to that path, answer "why can't `ssh` do this?" There is no
in-process SSH client; anything in `docs/PLAN.md` about `russh` is history.

### Storage and migrations

SQLite at `<data dir>/kelyfos/app.db`, WAL mode, `foreign_keys = ON`.
`modules/storage/mod.rs` holds the CRUD commands; `migrations.rs` holds schema.

Two halves, and **a new column belongs in both**: `SCHEMA` (`CREATE TABLE IF NOT
EXISTS`, what a fresh install gets) and `MIGRATIONS` (ordered steps, the only
path an existing install takes). Miss the migration step and `row_to_*` fails on
`row.get("col")?`, which makes the whole list query return `Err` — an empty
sidebar, not a blank field. A shipped migration is immutable; fix it with a new
step. Full rules and the backup behaviour are in `README.md`.

### Terminal status resolution

`src/lib/agentStatus.ts` is pure and is the single source of truth for what dot a
terminal shows. Every bug this has had was a precedence mistake, so status
precedence lives there and nowhere else — do not re-derive it in a component.
Inputs are kept as separate maps on purpose (PTY status, agent snapshots,
title/bell heuristics, finished runs, unwatched output); folding them into one
field would mean whichever event landed last wins.

`modules/process` is the shared process table underneath both the busy fallback
and Claude presence, and the one place platform divergence is allowed to live:
one name normaliser, one interactive-program list, one busy classifier. Unix
answers "is this terminal busy" from the PTY's foreground process group (one
ioctl, per-session task); Windows has no foreground job, so an app-global poller
infers it from the shell's descendants. Keep the classifier free of `cfg` — it is
how macOS tests cover the Windows logic, and a Windows-only `cargo check` cannot
see unix-gated code, which is how the original Windows compile break shipped.

Two independent signals feed the agent side, both optional:

- **Shell integration** (`modules/shell_integration`) — OSC 133 marks give exact
  command start/exit. zsh gets a generated `ZDOTDIR` that sources the user's own
  rc files first; bash gets an `--init-file`. **Nothing the user owns is
  modified.** Any other shell (fish, nushell, Windows shells) runs unmodified
  with no integration. Bump `SCRIPTS_VERSION` when the generated scripts change.
- **Claude Code awareness** (`modules/agent`) — presence by process scan (no
  config), plus opt-in hooks. The bridge runs on both platforms over different
  transports: a Unix socket and `claude-hook.sh` on macOS/Linux, a named pipe
  and `claude-hook.ps1` on Windows. Only `socket_path`, `listening`, `start`,
  `shim_path`, `shim_source` and `hook_command` are `cfg`-split — the wrapper
  format, `handle` and the state machine are shared, and that is what keeps the
  trust rules from having two versions. Hook payloads are untrusted input:
  parsed, terminal id checked against live sessions, never executed. Windows
  costs ~170 ms per event to PowerShell startup, which is why the consent panel
  says so; shell integration is still unsupported there.

### Frontend structure

- `src/stores/*` — Zustand, one per domain. Every store calls `ipc.*`; components
  do not invoke directly. Boot order lives in `App.tsx`: settings → connections /
  presets / workspaces → `restoreSession()`.
- `src/components/layout/WorkspaceView.tsx` — hosts one FlexLayout `Model` per
  workspace, persisted verbatim into `workspaces.layout` and saved debounced
  (500 ms) through `useWorkspacesStore.saveLayout`.
- `src/lib/workspaceHandles.ts` — registry letting code outside the FlexLayout
  tree (quick search, Cmd+W) act on a workspace's grid.
- `src/components/common/Select.tsx` — every dropdown in the app; there are no
  `<select>` elements left. A native list is drawn outside the page and styled by
  the platform: Chromium switches to an author-styled popup (white by default) as
  soon as an option carries a style of its own, which the font picker's per-family
  preview does, and WKWebView hands the list to a native menu. Neither can be
  made to match, so the list is ours.
- `src/lib/outputHighlight.ts` — optional Termius-style colouring of URLs and IP
  addresses, done by rewriting the PTY stream on its way into xterm (nothing can
  recolour text already in the buffer). It is a full escape-sequence scanner
  because it has to be: it touches text only, only where the foreground is
  default, and never on the alternate screen. Read its header before widening
  what it matches.
- `src/components/terminal/XtermView.tsx` — the xterm lifecycle: listeners,
  clipboard/search key handling, and the ResizeObserver that ignores 0×0
  (an unselected tab is `display:none`; fitting then corrupts the WebGL atlas).
  Remounting it via `useTerminalsStore.reconnect` (epoch bump) is how a session
  is restarted.
- `src/styles/global.css` — all styling, driven by CSS variables that
  `applyUiTheme` (`src/lib/themes.ts`) sets from the active theme. FlexLayout's
  `dark.css` must be imported before it.

### Variables and presets

Resolution is global → workspace → terminal, later scopes overriding
(`modules/variables`), with `${VAR}` substitution; unknown names are left
untouched. Presets are recipes with their own `preset` scope: opening one *copies*
its variables onto the new terminal, so editing a preset never disturbs terminals
already on screen.

## Conventions

- Comments here explain *why*, often at length, and are load-bearing — several
  document decisions that were reverted once already (title-matching for status,
  output-matching for errors, `.ok()` on migration failures). Read them before
  changing the behaviour they guard, and keep the density when adding code.
- Errors that reach the user are `Result<_, String>` with a message written for
  them, not a debug dump.
- Anything Claude-related must degrade silently to "no Claude": a machine that
  has never heard of it must behave identically.

## Document status

- `README.md` — authoritative for migrations, backups/restore, terminal status
  semantics, the Claude hook bridge, and the release/updater process.
- `docs/windows-process-detection.md` — the Windows process layer: the original
  brief, annotated with what was built and where it deviated. **Implemented; not
  yet verified on macOS or by hand on Windows** — §7 lists exactly which criteria
  are still unticked. Read §4.2 and §6 before changing the Windows busy poller:
  they record why the walk stops at interactive programs and why the tick is
  gated on terminal output rather than running every 350 ms.
- `docs/PLAN.md` — original architecture plan. **Partly stale**: it describes an
  `ssh`/`russh` module and `ssh:*` events that no longer exist, and omits presets
  and the agent/shell-integration modules. Useful for intent, not as a map.
- `prompt.md` — the original product spec, historical.
