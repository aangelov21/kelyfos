//! The process table: who spawned whom, and what each one is.
//!
//! Two features need the same picture of the machine. Claude presence looks for
//! a `claude` among a terminal's descendants; the fallback busy indicator asks
//! whether anything is running in a terminal at all. Both walk down from a shell
//! pid, so they share one snapshot rather than taking one each — the reason this
//! lived inside `agent` before, and the reason it no longer does.
//!
//! The two platforms answer the busy question differently, and deliberately so.
//! Unix has a foreground process group: the terminal itself knows which job owns
//! the screen. That is an exact answer and costs one ioctl, so `pty` keeps asking
//! it and never comes here for that. Windows has no such concept — a ConPTY has
//! no foreground job and no controlling terminal — so the only answer available
//! there is inferred from the shell's descendants. What that costs in accuracy is
//! written up in `docs/windows-process-detection.md` §4.2; the short version is
//! that this file has to decide, by hand, which descendants "count".
//!
//! Everything platform-independent here (the normaliser, the interactive list,
//! the walk, the busy classifier) stays compiled on unix so that macOS `cargo
//! test` covers the Windows logic. A Windows-only `cargo check` cannot see
//! unix-gated code, and that asymmetry is how the original compile break shipped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a process-table snapshot may be reused. The agent poller runs every
/// 2000 ms and the Windows busy poller every 350 ms; when their ticks coincide
/// they must share one reading rather than taking two, and a table a quarter of
/// a second old cannot change either answer.
const CACHE_TTL: Duration = Duration::from_millis(250);

/// A corrupt table could describe a cycle, and no shell legitimately has this
/// many descendants.
const MAX_NODES: usize = 4096;

/// Every helper process the app runs for itself — `git` for the branch pill,
/// `claude --version`, the hook self-test — goes through here rather than
/// `Command::new` directly.
///
/// On Windows a console program spawned by a GUI process gets a console of its
/// own, and it is a real window: it appears, takes focus for a few milliseconds
/// and vanishes. The status bar asks for the git branch every five seconds per
/// terminal, so four open tabs meant a black rectangle flickering over the
/// user's screen more or less permanently. CREATE_NO_WINDOW suppresses it.
///
/// Invisible during development, which is why it shipped: `cargo run` and
/// `tauri dev` give the app a console, and children attach to that one instead
/// of creating their own. It only appears in a `windows_subsystem = "windows"`
/// build — that is, only for the people you send the installer to.
///
/// This deliberately does not cover the PTY (a terminal is supposed to have a
/// console) or the hook shim, which Claude spawns from its own console.
pub fn command(program: &str) -> std::process::Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// windows_sys has this as a bare constant in Win32_System_Threading,
        /// a feature this crate does not otherwise need. One documented value,
        /// stable since NT, is not worth the extra compile.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Last snapshot and when it was taken. The `Option` inside is kept so a failed
/// read is cached too: a machine where the table cannot be read at all must not
/// be asked again every 350 ms.
static CACHE: Mutex<Option<(Instant, Option<Arc<ProcessTree>>)>> = Mutex::new(None);

/// A snapshot of the process table, so descendant lookups for every terminal
/// cost one reading rather than one per terminal.
pub struct ProcessTree {
    /// parent pid -> [(child pid, normalised command name)]
    children: HashMap<u32, Vec<(u32, String)>>,
}

/// What the walk should do after looking at a process.
enum Walk {
    /// Look at this process's own children too.
    Descend,
    /// Ignore this subtree entirely.
    Skip,
    /// Stop the whole walk.
    Stop,
}

impl ProcessTree {
    /// A process table at most `CACHE_TTL` old, read now if the last one has
    /// expired. None where the table cannot be read.
    pub fn snapshot() -> Option<Arc<ProcessTree>> {
        let mut cache = CACHE.lock().ok()?;
        if let Some((at, tree)) = cache.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return tree.clone();
            }
        }
        // Read under the lock on purpose: two pollers ticking together should
        // queue behind one reading, not take one each.
        let tree = Self::read().map(Arc::new);
        *cache = Some((Instant::now(), tree.clone()));
        tree
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn read() -> Option<Self> {
        let out = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut children: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(ppid), Some(comm)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) else {
                continue;
            };
            children
                .entry(ppid)
                .or_default()
                .push((pid, normalize_name(comm)));
        }
        Some(Self { children })
    }

    /// Windows has no `ps`, and spawning anything every 350 ms would cost far
    /// more than the answer is worth. `sysinfo` reads the table through
    /// `NtQuerySystemInformation` with no child process involved.
    ///
    /// This is the expensive call in the whole feature — about 7 ms on a machine
    /// with 500 processes, and a `CreateToolhelp32Snapshot` was measured at the
    /// same cost, so there is no cheaper documented way to ask. That is why
    /// `pty::busy_tick` works hard not to call it: see the output gate there.
    #[cfg(windows)]
    fn read() -> Option<Self> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};

        let mut children: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
        {
            let mut guard = SYSTEM.lock().ok()?;
            let sys = guard.get_or_insert_with(sysinfo::System::new);
            // Only what the tree needs. `nothing()` still yields pid, parent and
            // name; asking for cwd, command line, user or memory here would
            // multiply a cost that is already the largest in this module.
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing(),
            );
            for (pid, proc_) in sys.processes() {
                let Some(ppid) = proc_.parent() else { continue };
                children
                    .entry(ppid.as_u32())
                    .or_default()
                    .push((pid.as_u32(), normalize_name(&proc_.name().to_string_lossy())));
            }
        }
        prune_claude_verdicts(&children);
        Some(Self { children })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn read() -> Option<Self> {
        None
    }

    /// Depth-first walk of everything below `root`. `visit` receives the child's
    /// pid, its normalised name and its depth below `root` (1 for a direct
    /// child), and decides whether the walk carries on into that subtree.
    fn walk<'a>(&'a self, root: u32, visit: &mut dyn FnMut(u32, &'a str, u32) -> Walk) {
        let mut stack: Vec<(u32, u32)> = vec![(root, 0)];
        let mut seen = 0usize;
        while let Some((pid, depth)) = stack.pop() {
            seen += 1;
            if seen > MAX_NODES {
                return;
            }
            let Some(kids) = self.children.get(&pid) else {
                continue;
            };
            for (child, name) in kids {
                match visit(*child, name.as_str(), depth + 1) {
                    Walk::Descend => stack.push((*child, depth + 1)),
                    Walk::Skip => {}
                    Walk::Stop => return,
                }
            }
        }
    }

    /// Whether any descendant of `root` has this exact (normalised) name.
    pub fn has_descendant(&self, root: u32, name: &str) -> bool {
        self.has_descendant_matching(root, |_, n| n == name)
    }

    /// Whether any descendant of `root` satisfies `pred`, which receives the pid
    /// as well as the name — Claude Code on Windows cannot be recognised from
    /// the name alone, see [`is_claude`].
    pub fn has_descendant_matching(&self, root: u32, pred: impl Fn(u32, &str) -> bool) -> bool {
        let mut found = false;
        self.walk(root, &mut |pid, name, _| {
            if pred(pid, name) {
                found = true;
                Walk::Stop
            } else {
                Walk::Descend
            }
        });
        found
    }
}

/// The deepest descendant of `root` that looks like a command still working, or
/// None when the shell is sitting at its prompt.
///
/// Deepest rather than nearest because `pty_cwd` wants the directory the user is
/// actually in: a build script that has descended into a subfolder should hand
/// back the subfolder, not the script's own starting point.
///
/// Two kinds of subtree are cut off:
///
///   * an interactive program owns the screen for as long as the user wants it,
///     and whatever it spawns is its own business rather than the terminal's.
///     Cutting there mirrors unix, where the foreground process group *is* the
///     interactive program and its children are invisible to the indicator — so
///     Claude Code running a tool does not flash the busy dot on Windows either.
///   * anything `detached` reports. On Windows that is a process owning a
///     visible window: `Start-Process notepad` returns to the prompt at once but
///     leaves a descendant behind, and a tab that stays busy until the user
///     closes Notepad is exactly the dot that cries wolf.
///
/// Kept free of `cfg` so macOS tests cover it; unix answers the busy question
/// from the foreground process group and never calls this.
pub fn running_command_pid(
    tree: &ProcessTree,
    root: u32,
    detached: &dyn Fn(u32) -> bool,
) -> Option<u32> {
    let mut best: Option<(u32, u32)> = None; // (depth, pid)
    tree.walk(root, &mut |pid, name, depth| {
        if detached(pid) || is_interactive_program(name) {
            return Walk::Skip;
        }
        if best.is_none_or(|(d, _)| depth > d) {
            best = Some((depth, pid));
        }
        Walk::Descend
    });
    best.map(|(_, pid)| pid)
}

/// The comparable name of an executable, so that one list of program names works
/// on every platform.
///
/// Windows reports `Node.exe` where unix reports `node`; `ps` prints a full path
/// for anything off the default PATH (exactly where claude installs itself) and
/// a leading `-` for a login shell. All of that has to come off before a name can
/// be compared with anything, and it has to come off the same way on both
/// platforms or the two program lists quietly drift apart.
pub fn normalize_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let lower = base.trim_start_matches('-').to_ascii_lowercase();
    // Windows carries the extension in the process name itself. These four are
    // the ones that turn up as an executable in a process table.
    for ext in [".exe", ".com", ".bat", ".cmd"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    lower
}

/// Programs that sit in the foreground for as long as the user wants them and
/// are not "a command that is still working". Only consulted by the fallback
/// poller — shell integration knows the difference without a list.
///
/// Names arrive normalised, so `Vim` and `powershell.exe` reach this as `vim`
/// and `powershell` and one list serves both platforms.
pub fn is_interactive_program(name: &str) -> bool {
    matches!(
        name,
        "claude"
            | "vim"
            | "nvim"
            | "vi"
            | "emacs"
            | "nano"
            | "less"
            | "more"
            | "man"
            | "top"
            | "htop"
            | "btop"
            | "ssh"
            | "mosh"
            | "tmux"
            | "screen"
            | "lazygit"
            | "python"
            | "python3"
            | "node"
            | "irb"
            | "psql"
            | "mysql"
            // Windows shells the user drops into the way they would type `bash`
            // on unix. These four names cannot occur on macOS, so adding them
            // cannot change what macOS does — which is why `bash`, `sh` and
            // `zsh` are *not* here even though Git Bash would benefit: adding
            // them would silence the busy dot for `bash deploy.sh` on macOS too.
            // A batch file reaching node through cmd.exe is hidden by this, in
            // the same way `npm install` is already hidden by `node` above.
            | "pwsh"
            | "powershell"
            | "cmd"
            | "wsl"
    )
}

/// Whether a process in a terminal's tree is Claude Code.
///
/// On unix the binary is `claude` and that is the whole test. On Windows an npm
/// install runs as `node.exe` with the CLI's entry point as an argument, so
/// there is no `claude.exe` in the tree at all — matching on the name alone is
/// why Windows has never shown the Claude mark. The command line is read only
/// for processes already named `node`, because it costs a read of the target
/// process's memory, and the verdict is cached per pid since a command line
/// never changes.
pub fn is_claude(pid: u32, name: &str) -> bool {
    if name == "claude" {
        return true;
    }
    #[cfg(windows)]
    if name == "node" {
        return node_is_claude(pid);
    }
    let _ = pid; // unix never needs it, and the signature is shared
    false
}

/// Whether a `node` command line is running Claude Code. Matched on the entry
/// point rather than the word `claude` appearing anywhere, so that a node
/// process merely working on a file called `claude.md` does not light the mark.
#[cfg(any(windows, test))]
fn cmdline_is_claude(args: &[String]) -> bool {
    // Skip argv[0]: that is the node binary's own path, and a user whose home
    // directory is called `claude` would otherwise match every node process.
    args.iter().skip(1).any(|arg| {
        let arg = arg.to_ascii_lowercase();
        arg.contains("claude-code")
            || arg.contains("@anthropic-ai")
            || arg.ends_with("/claude")
            || arg.ends_with("\\claude")
            || arg.ends_with("claude.js")
    })
}

// ---------- Windows ----------

/// One `System` reused across readings. `sysinfo` diffs against what it already
/// knows, so keeping it costs a little memory and saves most of the work on
/// every refresh after the first.
#[cfg(windows)]
static SYSTEM: Mutex<Option<sysinfo::System>> = Mutex::new(None);

/// pid -> "this node process is Claude Code". Pruned against each new table, so
/// it cannot grow without bound.
#[cfg(windows)]
static CLAUDE_VERDICTS: Mutex<Option<HashMap<u32, bool>>> = Mutex::new(None);

#[cfg(windows)]
fn node_is_claude(pid: u32) -> bool {
    // Two short lock sections rather than one: reading the command line takes
    // the SYSTEM lock, and holding this one across that would invite a deadlock
    // the first time anything else wants both.
    if let Ok(cache) = CLAUDE_VERDICTS.lock() {
        if let Some(known) = cache.as_ref().and_then(|m| m.get(&pid)) {
            return *known;
        }
    }
    let verdict = cmdline_is_claude(&command_line(pid));
    if let Ok(mut cache) = CLAUDE_VERDICTS.lock() {
        cache.get_or_insert_with(HashMap::new).insert(pid, verdict);
    }
    verdict
}

/// Forget verdicts for pids that are no longer in the table. Windows hands pids
/// out in multiples of four and cycles through them slowly, so a pid reused
/// between two readings — the one case this cannot catch — is both very unlikely
/// and self-correcting at the next reading.
#[cfg(windows)]
fn prune_claude_verdicts(children: &HashMap<u32, Vec<(u32, String)>>) {
    let Ok(mut cache) = CLAUDE_VERDICTS.lock() else {
        return;
    };
    let Some(map) = cache.as_mut() else { return };
    if map.is_empty() {
        return;
    }
    let live: std::collections::HashSet<u32> = children
        .values()
        .flat_map(|kids| kids.iter().map(|(pid, _)| *pid))
        .collect();
    map.retain(|pid, _| live.contains(pid));
}

/// The command line of one process. Deliberately a targeted refresh: asking for
/// command lines across the whole table would read every process's memory.
#[cfg(windows)]
fn command_line(pid: u32) -> Vec<String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

    let Ok(mut guard) = SYSTEM.lock() else {
        return Vec::new();
    };
    let sys = guard.get_or_insert_with(sysinfo::System::new);
    let pid = Pid::from_u32(pid);
    // `false`: this must not evict the rest of the table, which the tree reading
    // just filled in.
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
    );
    sys.process(pid)
        .map(|p| {
            p.cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Pids owning a visible top-level window.
///
/// This is the detached-process filter from `docs/windows-process-detection.md`
/// §4.2. There is no clean way to ask whether a pid is attached to *our* ConPTY
/// — `GetConsoleProcessList` only reports the caller's own console, and
/// `AttachConsole` would disrupt this process — so the question is turned round:
/// a process that put a window on screen is not a command running in a terminal.
/// `Start-Process notepad` is the case this exists for.
#[cfg(windows)]
pub fn windowed_pids() -> std::collections::HashSet<u32> {
    use std::collections::HashSet;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid != 0 {
                // Set up by the caller below and alive for the whole call.
                let set = unsafe { &mut *(lparam as *mut HashSet<u32>) };
                set.insert(pid);
            }
        }
        TRUE
    }

    let mut set: HashSet<u32> = HashSet::new();
    unsafe { EnumWindows(Some(collect), &mut set as *mut HashSet<u32> as LPARAM) };
    set
}

/// Working directory of a process. Read from the target's own PEB by `sysinfo`;
/// there is no Windows equivalent of `/proc/pid/cwd` or `lsof`.
#[cfg(windows)]
pub fn cwd_of_pid(pid: u32) -> Option<String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

    let mut guard = SYSTEM.lock().ok()?;
    let sys = guard.get_or_insert_with(sysinfo::System::new);
    let pid = Pid::from_u32(pid);
    // `Always`: a shell's directory changes with every `cd`, so a cached answer
    // is exactly the wrong one. This runs once per new terminal, not in a loop.
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
    );
    sys.process(pid)
        .and_then(|p| p.cwd())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(rows: &[(u32, u32, &str)]) -> ProcessTree {
        let mut children: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
        for (pid, ppid, comm) in rows {
            children
                .entry(*ppid)
                .or_default()
                .push((*pid, normalize_name(comm)));
        }
        ProcessTree { children }
    }

    /// Nothing is ever detached in these fixtures unless a test says so.
    fn attached(_pid: u32) -> bool {
        false
    }

    // ---------- normalisation ----------

    #[test]
    fn normalises_windows_extensions() {
        assert_eq!(normalize_name("NODE.EXE"), "node");
        assert_eq!(normalize_name("more.com"), "more");
        assert_eq!(normalize_name("npm.cmd"), "npm");
        assert_eq!(normalize_name("build.bat"), "build");
    }

    #[test]
    fn normalises_unix_shapes() {
        assert_eq!(normalize_name("node"), "node");
        assert_eq!(normalize_name("/usr/bin/node"), "node");
        assert_eq!(normalize_name("-zsh"), "zsh");
        assert_eq!(normalize_name("C:\\Windows\\System32\\cmd.exe"), "cmd");
    }

    /// A dot that is not one of the launcher suffixes stays put, or `python3.11`
    /// and `foo.sh` would lose half their name.
    #[test]
    fn keeps_other_dots() {
        assert_eq!(normalize_name("python3.11"), "python3.11");
        assert_eq!(normalize_name("deploy.sh"), "deploy.sh");
    }

    #[test]
    fn interactive_list_matches_both_name_shapes() {
        for raw in ["vim", "Vim", "/usr/bin/vim", "vim.exe"] {
            assert!(is_interactive_program(&normalize_name(raw)), "{raw}");
        }
        for raw in ["powershell.exe", "pwsh.exe", "cmd.exe", "NODE.EXE"] {
            assert!(is_interactive_program(&normalize_name(raw)), "{raw}");
        }
        for raw in ["cargo", "make", "sleep", "tsc.exe"] {
            assert!(!is_interactive_program(&normalize_name(raw)), "{raw}");
        }
    }

    // ---------- descendant lookups ----------

    #[test]
    fn finds_a_direct_child() {
        let t = tree(&[(200, 100, "claude")]);
        assert!(t.has_descendant(100, "claude"));
    }

    /// Claude launched through a wrapper still belongs to the terminal.
    #[test]
    fn finds_a_deep_descendant() {
        let t = tree(&[(200, 100, "npm"), (300, 200, "node"), (400, 300, "claude")]);
        assert!(t.has_descendant(100, "claude"));
    }

    #[test]
    fn does_not_find_a_sibling_terminals_process() {
        let t = tree(&[(200, 100, "vim"), (300, 999, "claude")]);
        assert!(!t.has_descendant(100, "claude"));
    }

    #[test]
    fn survives_a_cycle() {
        let t = tree(&[(100, 200, "a"), (200, 100, "b")]);
        assert!(!t.has_descendant(100, "claude"));
    }

    #[test]
    fn matching_survives_a_cycle() {
        let t = tree(&[(100, 200, "a"), (200, 100, "b")]);
        assert!(!t.has_descendant_matching(100, |_, n| n == "claude"));
    }

    #[test]
    fn empty_tree_finds_nothing() {
        assert!(!tree(&[]).has_descendant(1, "claude"));
    }

    /// The Windows shape: no `claude.exe` anywhere, only a node process.
    #[test]
    fn matching_can_use_the_pid() {
        let t = tree(&[(200, 100, "node.exe")]);
        assert!(!t.has_descendant(100, "claude"));
        assert!(t.has_descendant_matching(100, |pid, name| pid == 200 && name == "node"));
    }

    // ---------- Claude command lines ----------

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn recognises_an_npm_installed_claude() {
        assert!(cmdline_is_claude(&args(&[
            "C:\\Program Files\\nodejs\\node.exe",
            "C:\\Users\\u\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js",
        ])));
    }

    #[test]
    fn recognises_a_local_claude_entry_point() {
        assert!(cmdline_is_claude(&args(&["node", "/opt/claude"])));
        assert!(cmdline_is_claude(&args(&["node", "./bin/claude.js"])));
    }

    /// argv[0] is the node binary itself; a home directory called `claude`
    /// must not turn every node process into an agent.
    #[test]
    fn ignores_claude_in_argv0() {
        assert!(!cmdline_is_claude(&args(&["C:\\claude\\node.exe"])));
    }

    #[test]
    fn ignores_an_unrelated_node_process() {
        assert!(!cmdline_is_claude(&args(&["node", "vite", "--port", "1420"])));
        assert!(!cmdline_is_claude(&args(&["node", "notes/claude.md"])));
        assert!(!cmdline_is_claude(&[]));
    }

    // ---------- busy classification ----------

    #[test]
    fn idle_shell_is_not_busy() {
        let t = tree(&[(200, 999, "cargo")]); // someone else's build
        assert_eq!(running_command_pid(&t, 100, &attached), None);
    }

    #[test]
    fn a_running_command_is_busy() {
        let t = tree(&[(200, 100, "cargo.exe")]);
        assert_eq!(running_command_pid(&t, 100, &attached), Some(200));
    }

    /// The regression the OSC 133 work was built to kill: an editor holding the
    /// screen is not work in progress.
    #[test]
    fn an_interactive_program_is_not_busy() {
        let t = tree(&[(200, 100, "vim.exe")]);
        assert_eq!(running_command_pid(&t, 100, &attached), None);
    }

    /// Claude Code shells out constantly. On unix those grandchildren are inside
    /// claude's own process group and invisible to the indicator; cutting the
    /// subtree keeps Windows the same.
    #[test]
    fn work_inside_an_interactive_program_is_not_busy() {
        let t = tree(&[(200, 100, "claude"), (300, 200, "rg.exe")]);
        assert_eq!(running_command_pid(&t, 100, &attached), None);
    }

    #[test]
    fn a_detached_gui_process_is_not_busy() {
        let t = tree(&[(200, 100, "notepad.exe")]);
        assert_eq!(running_command_pid(&t, 100, &|pid| pid == 200), None);
    }

    /// A detached app's own children go with it; nothing under Notepad is the
    /// terminal's business either.
    #[test]
    fn a_detached_process_takes_its_subtree_with_it() {
        let t = tree(&[(200, 100, "notepad.exe"), (300, 200, "helper.exe")]);
        assert_eq!(running_command_pid(&t, 100, &|pid| pid == 200), None);
    }

    /// `pty_cwd` wants where the user ended up, not where the outermost command
    /// started.
    #[test]
    fn prefers_the_deepest_command() {
        let t = tree(&[(200, 100, "make.exe"), (300, 200, "cc.exe")]);
        assert_eq!(running_command_pid(&t, 100, &attached), Some(300));
    }

    // ---------- against the real machine (Windows) ----------
    //
    // The fixtures above prove the logic; these prove the platform code under it
    // actually reads this machine. They are cheap and deterministic — a spawned
    // pid exists the moment `spawn` returns — so they run in the normal suite
    // rather than hiding behind `--ignored`.

    /// §7.4 and §7.5 in miniature: a real command under a real parent is found
    /// in the real table, reads as busy, and stops reading as busy once it is
    /// gone. One test rather than three because the fixture is a child of the
    /// test binary itself — a second test spawning its own would be a sibling
    /// this one cannot tell apart from its own, and the two would race.
    #[cfg(windows)]
    #[test]
    fn finds_and_classifies_a_real_child() {
        // `ping` is on every Windows install, is not on the interactive list,
        // and outlives the few milliseconds this needs.
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");

        let me = std::process::id();
        let tree = ProcessTree::read().expect("process table");
        let windowed = windowed_pids();
        let found = tree.has_descendant(me, "ping");
        let busy = running_command_pid(&tree, me, &|pid| windowed.contains(&pid));

        child.kill().ok();
        child.wait().ok();
        let after = ProcessTree::read().expect("process table");
        let still_busy = running_command_pid(&after, me, &|_| false);

        assert!(found, "a child spawned by this test was not in the table");
        assert_eq!(busy, Some(child.id()), "the child should read as the command");
        assert_eq!(still_busy, None, "the dot should clear once the child exits");
    }

    /// What `pty_cwd` falls back to when the shell is idle. Reading a process's
    /// own directory is the part with no unix equivalent, so it is worth proving
    /// against a directory the test already knows.
    #[cfg(windows)]
    #[test]
    fn reads_a_processes_working_directory() {
        let expected = std::env::current_dir().expect("cwd");
        let read = cwd_of_pid(std::process::id()).expect("own cwd");
        assert_eq!(
            std::path::Path::new(&read),
            expected,
            "sysinfo read a different directory than the process is in"
        );
    }

    /// The numbers behind `docs/windows-process-detection.md` §6, and the way to
    /// get them again. Ignored by default: it measures the machine it runs on
    /// rather than asserting anything, so it would only ever be a flaky test.
    ///
    ///   cargo test --lib process::tests::measure -- --ignored --nocapture
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn measure_a_poll_tick() {
        use std::time::Instant;

        // The first reading fills sysinfo's table from empty and is not what the
        // poller pays after that.
        let _ = ProcessTree::read();

        let mut reads = Vec::new();
        let mut procs = 0;
        for _ in 0..20 {
            let at = Instant::now();
            let tree = ProcessTree::read().expect("process table");
            reads.push(at.elapsed());
            procs = tree.children.values().map(Vec::len).sum::<usize>();
        }
        reads.sort();

        let mut windows = Vec::new();
        for _ in 0..20 {
            let at = Instant::now();
            let found = windowed_pids();
            windows.push(at.elapsed());
            assert!(!found.is_empty(), "this desktop has no visible window");
        }
        windows.sort();

        // A whole tick for four terminals, in the shape `busy_tick` runs it: one
        // reading, one window enumeration, then the walk for each shell.
        let shells: Vec<u32> = ProcessTree::read()
            .expect("process table")
            .children
            .keys()
            .take(4)
            .copied()
            .collect();
        let mut ticks = Vec::new();
        for _ in 0..20 {
            let at = Instant::now();
            let tree = ProcessTree::read().expect("process table");
            let windowed = windowed_pids();
            for shell in &shells {
                running_command_pid(&tree, *shell, &|pid| windowed.contains(&pid));
            }
            ticks.push(at.elapsed());
        }
        ticks.sort();

        println!("processes:            {procs}");
        println!(
            "ProcessTree::read()   median {:?}  worst {:?}",
            reads[10], reads[19]
        );
        println!(
            "windowed_pids()       median {:?}  worst {:?}",
            windows[10], windows[19]
        );
        println!(
            "full tick, 4 shells   median {:?}  worst {:?}",
            ticks[10], ticks[19]
        );
    }

    #[test]
    fn classification_survives_a_cycle() {
        let t = tree(&[(100, 200, "a.exe"), (200, 100, "b.exe")]);
        // Terminating is the whole assertion; the answer itself is meaningless
        // for a table that cannot be true.
        running_command_pid(&t, 100, &attached);
    }
}
