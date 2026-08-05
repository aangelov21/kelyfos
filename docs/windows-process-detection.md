# Task: make the PTY process layer work on Windows

**Status: implemented, 2026-07-31.** The crate compiles, `cargo test` passes (93 tests) and the app
runs on Windows for the first time. Every §7 item that can be checked without a human at the keyboard
or a mac has been; the ones that cannot are listed, unticked, in §7. Deviations from the design below
are marked **[changed]** in place, with the reason.

Read this whole file before editing. It records why the obvious cheap fix is the wrong one, and §4.2
and §6 record decisions that are now load-bearing in the code.

**What landed:** a new `src-tauri/src/modules/process` module holding the process table, the name
normaliser, the interactive-program list, the Claude predicate and the busy classifier; an app-global
busy poller in `pty` for Windows; `pty_cwd` and Claude presence working on Windows. `agent`'s
`ProcessTree` moved into the new module. No frontend change, no IPC change.

## 1. What is broken

`cargo test` on `x86_64-pc-windows-msvc` (Rust 1.97.1, VS Build Tools 2026, Windows SDK 10.0.26100)
compiles ~200 dependency crates successfully and then fails on our own code:

```
error[E0599]: no method named `process_group_leader` found for struct
              MutexGuard<'_, Box<dyn MasterPty + Send>>
  --> src/modules/pty/mod.rs:291
  --> src/modules/pty/mod.rs:499
error: could not compile `kelyfos` (lib) due to 2 previous errors
```

`MasterPty::process_group_leader` is declared `#[cfg(unix)]` in the trait itself
(`portable-pty-0.9.0/src/lib.rs:105-107`), alongside `as_raw_fd` and `tty_name`. It does not exist on
Windows, because Windows has no process groups and no controlling-terminal concept for a ConPTY. This
is not something a newer portable-pty version will add.

Two call sites depend on it:

| Site | Feature it powers | Windows impact today |
|---|---|---|
| `pty/mod.rs:288-291` | Fallback busy indicator: is a command running in this terminal? | no busy dot ever |
| `pty/mod.rs:499` (`pty_cwd`) | New terminal inherits the active pane's directory | new terminals always open in home |

Three further Windows gaps share the same root cause and must be fixed in the same pass, because they
are the same missing capability wearing different hats:

- **`ProcessTree::read()` returns `None` on Windows** (`agent/mod.rs:228-231`). It shells out to
  `ps -axo pid=,ppid=,comm=`, which does not exist on Windows. Consequence: Claude Code presence
  detection silently never fires, so no terminal ever shows the Claude mark, and — see
  `agent/mod.rs:158-163` — hook state can never be reconciled against reality.
- **`process_name()` returns `None` on Windows** (`pty/mod.rs:559-562`). Same `ps` dependency.
- **`cwd_of_pid()` returns `None` on Windows** (`pty/mod.rs:585-588`).

Those three already have honest `#[cfg]` fallbacks, which is why they compile. The two
`process_group_leader` sites were simply missed. Note that `pty_cwd`'s own doc comment
(`pty/mod.rs:486-490`) already promises "None ... or an unsupported platform", so the intent to
support Windows degradation was there from the start; only the wiring is absent.

## 2. Non-negotiables

1. **Do not stub `process_group_leader` to `None` and call it done.** Making it compile by returning
   `None` on Windows is a two-line change that leaves every Windows user with no busy dot, no cwd
   inheritance, and no Claude detection. That is explicitly rejected. Implement the real capability.
2. **macOS must not regress.** It is the only platform that currently works. After your change,
   every macOS behaviour described in `README.md` under "Terminal status" must be byte-for-byte what
   it was. See §7 for how to prove it.
3. **The unix hot path stays as it is.** On unix, busy detection costs one ioctl
   (`process_group_leader`) per 350 ms tick per terminal, and a `ps` spawn only when the foreground
   pid actually changes. That is already optimal and, more importantly, `process_group_leader` is
   *semantically better* than anything available on Windows: it reports the true foreground job.
   Do not replace it with tree-walking "for symmetry". Symmetry here would mean downgrading macOS.
4. **Keep the existing comment density.** Comments in this repo explain *why*, and several document
   decisions that were reverted once already. Your platform-divergence reasoning belongs in the code,
   not only in this file.

## 3. Design

The shape to aim for: **one shared process-table abstraction, two platform-specific answers to
"is this terminal busy?"**

### 3.1 Promote `ProcessTree` into its own module

Move `ProcessTree` out of `agent/mod.rs` (currently `agent/mod.rs:195-255`) into a new
`src-tauri/src/modules/process/mod.rs`, add it to `modules/mod.rs`, and have both `agent` and `pty`
consume it. It already has the right shape — `parent pid -> [(child pid, name)]` plus
`has_descendant` with a 4096-node cycle guard — and its whole reason for existing is stated in its
doc comment: "so descendant lookups for every terminal cost one `ps` rather than one per terminal".
That reasoning is what the Windows busy poller needs too.

Extend it with what the new callers need:

- `fn descendants(&self, root: u32) -> impl Iterator<Item = (u32, &str)>` — for classifying what is
  running, not just testing for one name.
  **[changed] Not added.** The only caller this was for is the busy classifier, and it needs to prune
  subtrees as it goes (an interactive program's children are not the terminal's business, §4.2), which
  a flat iterator cannot express. Both real callers go through one private `walk` with a
  `Descend / Skip / Stop` visitor instead, so the cycle guard lives in exactly one place. Adding
  `descendants` with no caller would have been dead code, and `cargo check` says so.
- `fn has_descendant_matching(&self, root: u32, pred: impl Fn(&str) -> bool) -> bool` — see §4.3 for
  why `claude` cannot be matched by exact name on Windows.
  **[changed]** the predicate takes `(pid, &str)`, not `&str` alone. The Windows Claude test needs the
  pid to read that process's command line.
- A short-TTL cache around `read()` (~250 ms). The agent poller runs at 2000 ms
  (`agent/mod.rs:37`) and the Windows busy poller wants ~350 ms; when their ticks coincide they must
  share one snapshot rather than taking two. Keep the cache internal to the module so callers cannot
  get it wrong.
  Done, as `ProcessTree::snapshot()`. `read()` stays private and the cache holds the `Option` too, so
  a machine where the table cannot be read at all is not asked again every tick.

### 3.2 Split the busy question by platform

Introduce one function that both platforms implement, and put the divergence behind it:

```rust
/// Whether a command is running in this terminal, as opposed to the shell sitting
/// at its prompt or a long-lived interactive program holding the screen.
///
/// unix answers this exactly, from the terminal's foreground process group.
/// Windows has no foreground job, so it infers from the shell's descendants —
/// see docs/windows-process-detection.md §4.2 for what that costs in accuracy.
fn is_running_command(...) -> bool
```

- **unix impl:** exactly today's logic from `pty/mod.rs:288-312`, unchanged, including the
  `last_fg` / `last_fg_interactive` memoisation that avoids a `process_name` spawn per tick.
- **Windows impl:** take a `ProcessTree` snapshot, walk down from `shell_pid`, and report running if
  any descendant is not an interactive program.

`shell_pid` is available on Windows: `child.process_id()` (`pty/mod.rs:213`) resolves to
`GetProcessId` in portable-pty's ConPTY child (`portable-pty-0.9.0/src/win/mod.rs:109-116`), so the
tree root is real. Note the ConPTY host process (`conhost.exe` / `OpenConsole.exe`) is *not* a child
of the shell, so it will not pollute the walk.

Keep the guards that wrap the poller in `pty/mod.rs:267`: `connection_id.is_none()` (an SSH terminal's
local process is `ssh` for the whole session, so this can only lie) and `!integrated` (shell
integration answers exactly when available). On Windows `integrated` is always false, because
`shell_integration::detect` maps every Windows shell to `ShellKind::Unsupported`
(`shell_integration/mod.rs:31-41`) — which means **this poller is the only busy signal Windows will
ever have.** Treat its accuracy as user-facing, not as a fallback nobody sees.

### 3.3 Where the Windows poller should live

On unix the poller is per-session and costs nothing shared, so it stays per-session. On Windows each
tick needs a process-table snapshot, and N per-session tasks would take N snapshots per tick. Prefer
**one app-global poller task** that iterates the open sessions per tick, in the manner
`agent::poll_once` (`agent/mod.rs:137-193`) already does — read the session list under the
`PtyManager` lock, release it, then evaluate. Respect the `instance` guard (`pty/mod.rs:283-287`):
a reconnect replaces a session while the old one is still tearing down, and a stale task must not
emit status for the new one.

If you keep per-session tasks on Windows instead, the TTL cache from §3.1 makes it correct but not
free. Measure before choosing (§6).

## 4. Windows implementation details

### 4.1 How to enumerate processes

Requirements: pid, parent pid, executable name for every process, plus cwd for one specific pid.
Must not spawn a child process — this runs every 350 ms.

**Recommended: the `sysinfo` crate.** One dependency covers process enumeration *and* cwd. Its
Windows backend uses `NtQuerySystemInformation`, which is faster than `CreateToolhelp32Snapshot` at
realistic process counts, and it reads cwd from the target process's PEB
(`NtQueryInformationProcess` + `ReadProcessMemory` + `RTL_USER_PROCESS_PARAMETERS`) — correct, but
fiddly enough by hand (WOW64 offset differences, undocumented struct layout) that hand-rolling it is
not a good trade.

Constrain what it gathers or it will be slower than the thing it replaced:

- Poll path: refresh with a `ProcessRefreshKind` limited to what the tree needs (parent + name).
  Do **not** request cwd, cmdline, user, disk or memory stats on the hot path.
- `pty_cwd` path: a targeted refresh of the single pid, with cwd requested. This runs once per new
  terminal, so it is allowed to be slower.

**Pin the version you resolve and read that version's docs.** `sysinfo`'s API has churned across
recent minor versions (`refresh_processes` signature, `ProcessesToUpdate`, `ProcessRefreshKind::new()`
vs `nothing()`). Do not copy an API shape from memory or from a blog post.

**Alternative if measurement rejects sysinfo:** `windows-sys` with only the ToolHelp feature, plus
Toolhelp32 snapshot for the tree. Lighter compile, but then cwd needs the PEB read by hand. Only take
this path with numbers to justify it, and record them here.

**Tried, and rejected on its own terms.** A `CreateToolhelp32Snapshot` tree was written and measured
against sysinfo on the same machine in the same run: 6.7 ms against 7.7 ms in one run, 6.8 ms against
6.8 ms in the next — a wash, not a justification. Splitting it further, `CreateToolhelp32Snapshot`
alone costs 4.5 ms and the bare `Process32NextW` loop another 2.2 ms, before any allocation of ours;
building the `HashMap` and normalising 500 names is lost in the noise. So the ~7 ms is the cost of
asking Windows the question, not the cost of either wrapper, and there is no cheaper documented API to
switch to. The ToolHelp code was deleted and sysinfo does both halves, exactly as recommended above.
That number is what forced the design in §6.

Resolved versions: `sysinfo = "0.39.6"` with `default-features = false, features = ["system"]`, and
`windows-sys = "0.61"` (already in the lock file via tauri) with `Win32_Foundation` and
`Win32_UI_WindowsAndMessaging` for the window filter in §4.2. Both Windows-only, so macOS compiles
neither.

Whichever you choose, add the dependency as **Windows-only** in `Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
sysinfo = "..."
```

That is the single most effective way to guarantee non-negotiable #2: a dependency macOS never
compiles cannot change macOS behaviour, and it keeps macOS build times untouched.

### 4.2 The accuracy cost, and the UX decision it forces

Windows has no foreground job. "A descendant is running" and "a command is running in the foreground"
are not the same statement, and they diverge in a case users will hit:

```powershell
Start-Process notepad          # or: Start-Job, or & in pwsh 7
```

The shell returns to its prompt immediately, but a descendant lives on, so a naive tree-walk reports
**busy forever**. That is precisely the failure mode `README.md` says the OSC 133 work existed to
kill ("the older approach watched the foreground process group, which reports a long-lived TUI as
busy for as long as it is open"), and the error-dot section states the principle plainly: "a dot that
cries wolf is worse than no dot."

So decide deliberately, and write the decision into the code as a comment:

- **Option A — detached-process filter.** Treat a descendant as foreground only if it is
  console-attached. There is no clean API for "is this pid attached to that ConPTY"
  (`GetConsoleProcessList` only reports the *caller's* console, and `AttachConsole` would disrupt our
  own process), so this ends up as heuristics: skip GUI processes (a process with a visible top-level
  window, via `EnumWindows` + `GetWindowThreadProcessId`), skip known launchers. Better UX, more code,
  more ways to be subtly wrong.
- **Option B — direct-child-only walk with a settle delay.** Only consider direct children of the
  shell, and require a descendant to persist across two consecutive ticks before reporting busy.
  Cheap, kills the flicker from short-lived helper processes, still wrong for `Start-Process`.
- **Option C — accept it, and document it.** Report busy for any non-interactive descendant. Simplest
  and never *misses* a running command; the `Start-Process` false positive stands.

**Recommendation: A layered on B** — direct-children-plus-descendants walk with the GUI-window filter,
because a detached GUI app is the only common false positive and it is cheap to exclude. If you cannot
make the GUI filter reliable, ship B and record the limitation in `README.md` under "Terminal status"
rather than leaving users to discover it. Do not ship C silently.

**Shipped: A layered on B, as recommended.** `process::windowed_pids()` is `EnumWindows` +
`IsWindowVisible` + `GetWindowThreadProcessId`, and any pid in that set has its whole subtree skipped.
It costs 32 µs — three orders of magnitude below the table reading — and it is skipped entirely unless
something already looks busy, since it can only ever remove candidates. B is the two-consecutive-ticks
settle in `pty::busy_tick`, which also buys a GUI app the ~700 ms it needs to get its window on screen
before the filter can recognise it. The limitation that survives both — a *console* app started
detached, with no window and no recognised name — is written up in `README.md` under "Terminal
status", along with the settle delay and the interactive-list false negatives.

One rule beyond the three options above, because Claude Code forced it: **the walk stops at an
interactive program** rather than descending through it. Claude shells out constantly, and on unix
those grandchildren sit inside claude's own process group and are invisible to the indicator. Without
the cut, Windows would flash the busy dot on every tool call while macOS showed nothing.

### 4.3 Name normalisation, and why `claude` will not match

Windows process names carry an extension: `powershell.exe`, `node.exe`, `vim.exe`, `more.com`. The
existing comparisons assume unix `comm` output:

- `is_interactive_program` (`pty/mod.rs:507-536`) matches bare names (`"vim"`, `"claude"`, `"node"`).
- `agent`'s presence check is `tree.has_descendant(*pid, "claude")` (`agent/mod.rs:159`), an exact
  string equality against `"claude"`.

Both need a shared normaliser: lowercase, strip a trailing `.exe` / `.com` / `.bat` / `.cmd`, and on
unix keep the existing behaviour of taking the last path segment and stripping a leading `-` (login
shells). Put it in the new `process` module and use it on both platforms so the two lists cannot
drift apart.

Then extend `is_interactive_program` with the Windows-only long-lived programs: `pwsh`, `powershell`,
`cmd`, `wsl`, `bash` (Git Bash), and confirm the existing entries still make sense once normalised.

**[changed] `bash` was left off.** `pwsh`, `powershell`, `cmd` and `wsl` are names that cannot occur on
macOS, so adding them cannot change what macOS does. `bash` can and does: adding it would silence the
busy dot for `bash deploy.sh` on macOS, which is non-negotiable #2. The consequence is that Git Bash
nested inside PowerShell reads as a running command and holds the dot — the same thing macOS does
today when you type `bash` inside zsh, so at least the two platforms are wrong in the same way. If
this is worth fixing it should be fixed for both at once, deliberately, not smuggled in here.

One more note on the normaliser: it lowercases, which it must for `Node.exe`. On macOS that means a
program named `Vim` now matches the interactive list where it did not before. This is the single
behaviour change on macOS in this whole change, it can only ever add matches to a list of
all-lowercase names, and the alternative was two normalisers that drift.

**The `claude` detection needs more than normalisation.** On Windows, Claude Code installed via npm
runs as `node.exe` executing a JS entry point; there is no `claude.exe` in the tree. Exact-name
matching therefore finds nothing, which is why Windows shows no Claude mark today. Fix it by matching
either:

- a process whose normalised name is `claude` (native installer), **or**
- a `node` process whose command line contains `claude` (npm install). `sysinfo` exposes `cmd()`;
  gathering it is more expensive than name-only, so gather it **only** for `node`-named candidates,
  and cache the verdict per pid — a pid's command line never changes.

Check what the native Windows installer actually produces before assuming; if it is `claude.exe`, the
first branch is the common case and the `node` branch is the fallback.

**Checked on the dev machine: it is `claude.exe`**, at `%USERPROFILE%\.local\bin\claude.exe`, so the
first branch is indeed the common case and `node` is the fallback for an npm install. Normalisation
turns `claude.exe` into `claude` and the existing test does the rest.

The `node` fallback matches the entry point, not the word `claude` anywhere on the command line, and
the reason is on this machine too: it runs a dozen `node.exe` MCP servers, one of them
`node C:/…/.claude/mcp/remote-logs/index.js`. A `contains("claude")` would mark that terminal as
running Claude Code. The predicate looks for `claude-code`, `@anthropic-ai`, a path segment ending in
`claude`, or `claude.js`, and skips argv[0] so a user whose home directory is called `claude` does not
match every node process on the box. All of those cases are unit-tested.

Beware the interaction with `is_interactive_program`: `"node"` is on that list, so a Claude session
correctly does not count as a running *command*. Keep that true.

### 4.4 `pty_cwd`

Return the cwd of the shell's most relevant descendant, falling back to the shell itself. On unix this
is the foreground process group leader; on Windows, prefer the deepest non-interactive descendant, and
fall back to `shell_pid` when the shell is idle at its prompt — which is the common case for this
feature, since the user presses "new terminal" while nothing is running.

Do **not** touch macOS's `cwd_of_pid` (`pty/mod.rs:573-583`, the `lsof -a -d cwd -Fn -p` spawn).
It runs once per new terminal, not in a loop, so its ~50 ms cost is invisible, and replacing a working
platform's implementation to gain nothing is how non-negotiable #2 gets broken. If you find sysinfo
would also serve macOS, note it here as possible future work and leave it alone.

## 5. Explicitly out of scope

Do not start these. They are separate pieces of work and folding them in will make this change
unreviewable:

- **The Claude hook bridge on Windows.** `hooks::socket_path()` returns `None` off unix
  (`agent/hooks.rs:47-52`) and the listener is `#[cfg(unix)]` with a `#[cfg(not(unix))]` no-op at
  `agent/hooks.rs:97`. Supporting Windows means a named pipe and a `.cmd`/PowerShell shim instead of
  `claude-hook.sh`. Presence detection — which *is* in scope — is what makes the Claude mark appear;
  the hook bridge is only what tells *working* from *waiting for you*.

  **[done, 2026-07-31 — separately, after this task]** Built as described: a named pipe at
  `\\.\pipe\kelyfos-hooks-<user>` and a `claude-hook.ps1`. Two notes for whoever reads this next.
  The `.cmd` option in the sentence above was measured (23 ms, against PowerShell's 171 ms) and
  **rejected** — batch has no way to relay stdin except `more`, a pager that injected a BOM into the
  middle of the payload and mangles long lines and non-ASCII. The 7× win is not worth a bridge that
  corrupts payloads. And the pipe server, unlike a socket listener, handles one client per instance,
  so the loop creates the next instance *before* handing off the connected one; Claude fires hooks in
  bursts and a client arriving with no instance waiting is refused outright. See `README.md`.
- **Shell integration for PowerShell.** OSC 133 marks from a `prompt` function would make §4.2's
  whole problem disappear on Windows, and is the better long-term answer. It is also a much larger
  change with its own `SCRIPTS_VERSION` migration story (`shell_integration/mod.rs:21`). If you do
  this later, the poller from this task becomes the fallback it was always meant to be.
- Replacing the unix `ps` spawn in `ProcessTree::read()` with sysinfo.
- Anything in the frontend. `src/lib/agentStatus.ts` already handles every status this produces, and
  its precedence rules are load-bearing — see the warning in `CLAUDE.md`.

## 6. Measure, do not guess

The poller runs every 350 ms for the life of every terminal, so its cost is the one number that
matters. Before settling on an enumeration approach, record on Windows with a realistic process count
(300+ processes, 4+ open terminals):

- wall time of one `ProcessTree::read()`,
- wall time of one full poll tick across all open sessions,
- idle CPU of the app with 4 terminals open and nothing running.

Put the numbers in this file when you are done. If a tick costs more than ~5 ms, either widen the
Windows interval (500 ms is still responsive for a busy dot) or switch enumeration strategy — but say
which, and why, in the code.

### Measured, 2026-07-31

Release build, this dev machine, 491–496 processes. Median of 20, worst of 20. Reproduce with:

```sh
cd src-tauri && cargo test --release --lib process::tests::measure -- --ignored --nocapture
```

| | median | worst |
|---|---|---|
| `ProcessTree::read()` (sysinfo) | 7.5 ms | 11.3 ms |
| `ProcessTree::read()` (ToolHelp, rejected) | 6.7–6.8 ms | 8.4 ms |
| `windowed_pids()` | 32 µs | 434 µs |
| full tick, 4 shells | 7.8 ms | 9.4 ms |

App idle CPU, debug build, window open, **no terminals**: 0.94% of one core over 20 s. The
four-terminal figure §6 asks for needs a human to open four terminals and is not measured (§7).

**The tick costs ~7.8 ms, well over the 5 ms line, and neither available API is cheaper (§4.1).** So
neither of the two suggested remedies was enough on its own: at 350 ms that is 2.2% of a core burned
forever by an app whose terminals are sitting at a prompt, and widening to 500 ms only takes it to
1.6%. Both are far too much for a status dot on a laptop.

**What was done instead: the tick reads the table only when something could have changed.** The gate
is in `pty::busy_tick` and it is three conditions —

- the terminal produced output since the last look (`SessionHandle::last_output_ms`, which already
  existed for the startup engine). A command cannot start without the shell echoing the line that
  started it, and cannot end without a prompt being drawn;
- **or** a command was already running last tick — `Start-Sleep 5` is silent for its whole run, so
  output alone would never see it finish, and the two-tick settle could never reach its second tick;
- **or** the terminal is new and has never been looked at.

An idle terminal therefore costs one atomic load per tick and no table reading at all, which is why
the interval stays at 350 ms and matches unix: the interval only decides responsiveness now, not idle
cost. The remaining steady-state cost is a burst of ~7.8 ms ticks while you are actually typing or
running something, which is the moment you are asking for the dot anyway.

## 7. Verification

Success is not "it compiles". Success criteria, in order.

**Status of each, 2026-07-31.** Ticked items were run on the Windows dev machine. Unticked ones need
either a human at the keyboard or a mac, and are the outstanding work on this change — nobody should
read "implemented" as "verified on both platforms".

- [x] 1, 2, 3 — `cargo test` 93 passed, `cargo check` clean with no warnings, `npm run build` clean
      and the IPC surface untouched. `cargo build` links and the app launches and stays up, which had
      never happened on this machine before.
- [ ] 4, 5, 6, 7, 8, 9 — all need someone to open terminals and watch dots.
- [ ] 10–16 — all need a mac.

Two automated stand-ins were added for the parts of 4, 5 and 7 that do not need a UI, and they run in
the normal suite on Windows rather than behind `--ignored`:

- `finds_and_classifies_a_real_child` spawns a real `ping`, asserts the real process table contains
  it, that the classifier reports exactly that pid as the running command, and that it reports nothing
  once the child is killed and reaped.
- `reads_a_processes_working_directory` asserts `cwd_of_pid` on the test's own pid equals its actual
  working directory — the PEB read is the piece with no unix equivalent, so it is worth proving.

The fixture-based tests below cover the platform-independent logic and run on macOS too, which was the
point of keeping the classifier free of `cfg`.

Original criteria:

**Windows** (`cargo test` and manual):

1. `cd src-tauri && cargo test` passes.
2. `cargo check --target x86_64-pc-windows-msvc` clean, no new warnings.
3. `npm run build` still clean (should be untouched; confirm you did not change the IPC surface).
4. Busy dot: run `Start-Sleep 5` in a terminal, watch the tab dot go busy then clear.
5. No false busy: open `vim` (or any entry in `is_interactive_program`), confirm the dot does **not**
   stick at busy for as long as it is open. This is the regression the OSC 133 work was built to kill.
6. Whatever §4.2 option you chose: run `Start-Process notepad`, confirm the behaviour matches what you
   documented, and that you did document it.
7. cwd inheritance: `cd` somewhere, open a new terminal in that workspace, confirm it starts there.
8. Claude mark: run `claude` in a terminal, confirm the mark appears within ~2 s (`POLL_MS`), and
   disappears within ~2 s of exiting. Then kill Claude outright (Task Manager) and confirm the mark
   clears rather than sticking — that reconciliation is the whole point of presence detection
   (`agent/mod.rs:158-181`).
9. A machine with no Claude installed behaves identically to before: no marks, no errors, nothing in
   the console. This is a stated invariant in `CLAUDE.md`.

**macOS** (must be run on a mac — a Windows `cargo check` cannot see unix-gated code, and that
asymmetry is exactly how this bug shipped in the first place):

10. `cd src-tauri && cargo test` passes, including the `agent`, `pty`, `migrations` and
    `shell_integration` test modules.
11. `cargo check` clean with no new warnings — especially no "unused" warnings from code that moved
    behind a `cfg`.
12. Busy dot with shell integration **off** (Settings → Terminal): `sleep 5` reports busy, `vim` does
    not. This is the code path you refactored; it is the one most likely to have rotted.
13. Busy dot with shell integration **on**: unchanged, OSC 133 still drives it, poller not involved.
14. Claude mark still appears and clears, and the hook bridge still reports working vs waiting
    (Settings → Claude Code should show all four ticks).
15. cwd inheritance still works.
16. An SSH terminal still shows no local busy dot (the `connection_id.is_none()` guard).

Add unit tests for everything platform-independent, so macOS CI covers your Windows logic:

- name normalisation: `"NODE.EXE"`, `"node"`, `"/usr/bin/node"`, `"-zsh"`, `"more.com"`.
- `is_interactive_program` against both name shapes.
- the busy classification given a synthetic `ProcessTree` — build the map directly rather than reading
  the real table, and assert idle-shell, running-command, interactive-program and detached-GUI cases.
- `has_descendant_matching` cycle guard still terminates.

All present in `modules/process`, plus: a detached process taking its subtree with it, work inside an
interactive program not counting, `prefers_the_deepest_command` for `pty_cwd`, and the four
`cmdline_is_claude` cases from §4.3. 22 fixture tests in all, none of them `cfg`-gated to Windows.

The existing tests in `pty/mod.rs:590+` cover `build_ssh_command` only; follow the local style
(`#[cfg(test)] mod tests` in the same file as the code it covers, fixtures built by hand).

## 8. Environment (already verified, 2026-07-31)

The Windows dev machine is set up; you do not need to install anything:

- Node v24.18.0, npm 12.0.1 — `npm install` done, `npx tsc --noEmit` clean, `npm run build` clean.
- Rust 1.97.1, host `x86_64-pc-windows-msvc`.
- Visual Studio Build Tools 2026 (18.8.12023.21) with `VC.Tools.x86.x64`, Windows SDK 10.0.26100.
- WebView2 runtime 150.0.4078.105.
- `cargo` is at `%USERPROFILE%\.cargo\bin`; a shell started before it was installed will not have it
  on PATH.

`npm run tauri dev` has never successfully run on this machine — the Rust crate has never compiled
here. Expect first-run surprises beyond the compile errors above, and note the compiler stops at the
first failing crate, so more Windows errors may surface once these two are past. Fix them in the same
spirit: real capability, no silent stubs.

**Update:** the two errors above were the only ones. `cargo build` links and `target/debug/kelyfos.exe`
starts, opens its window and stays up with nothing on stderr. Five pre-existing dead-code warnings did
surface, all from `agent/hooks.rs` — its payload parser is reachable only from the `#[cfg(unix)]`
socket listener, so Windows has no caller for any of it. Silenced with one file-level
`#![cfg_attr(not(unix), allow(dead_code))]` and a comment pointing at §5, rather than implementing the
named-pipe bridge that is explicitly out of scope. Without that, criterion §7.2 could never be met and
the next person would have no baseline to compare against.
