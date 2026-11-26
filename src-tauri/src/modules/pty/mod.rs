//! Local PTY session management built on portable-pty.
//! Each open terminal owns one local shell process on a real PTY, driven by a
//! reader thread plus a command task. The frontend talks to it through Tauri
//! commands; output and status flow back to it:
//!   output                   raw bytes, over the `Channel` handed to `pty_open`
//!   pty:status:{terminalId}  { status, message }, as a Tauri event
//!
//! Output is a channel rather than an event because an event is a JSON string:
//! every read used to be base64-encoded (a third larger), serialised, parsed and
//! decoded again. A channel delivers the bytes as an ArrayBuffer, in order, and
//! its pace is set by the frontend — see `Flow`.
//!
//! This is the primary session backend. A saved connection does not get its own
//! transport — the shell simply has an `ssh …` command typed into it, exactly as
//! a user would. When that command exits or fails, the local shell is still
//! there and still typeable, which the russh backend could never offer.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::modules::models::{AuthMethod, Connection, SessionCmd, StartupTask};
use crate::modules::process;
use crate::modules::shell_integration::{
    self,
    marks::{self, Mark, MarkScanner},
};
use crate::modules::storage::{self, Db};
use crate::modules::variables;

/// Distinguishes successive sessions for the same terminal. A reconnect spawns
/// a new session while the old one is still tearing down asynchronously, so
/// teardown must not evict whatever is in the map — only its own entry.
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Fallback busy poller interval, both platforms. Fast enough that the dot
/// tracks a command the user is watching; slow enough that the Windows tick,
/// which reads the whole process table rather than one ioctl, stays in the
/// noise — the numbers are in `docs/windows-process-detection.md` §6.
const POLL_MS: u64 = 350;

/// Bytes asked of the PTY per read. A read returns whatever is ready, so a
/// quiet terminal still gets each keystroke's echo on its own; only a flood
/// fills it, and then arrives in eight times fewer messages than at the old
/// 8 KiB for the webview to dispatch.
const READ_CHUNK: usize = 64 * 1024;

/// Read-side flow control for one session, driven by the frontend.
///
/// xterm parses what it is handed asynchronously and buffers the rest without
/// limit. So a `cat` of a large log used to be read off the PTY as fast as the
/// program could write it and queued in the webview, until the UI stalled and
/// memory climbed, and Ctrl+C took effect only after everything already queued
/// had been drawn. The frontend now counts the bytes xterm has not parsed yet
/// and calls `pty_pause` above a high-water mark, `pty_resume` below a low one.
/// While paused the reader stops reading: the PTY's own buffer fills and the
/// program blocks on its next write, which is what a real terminal does to a
/// program that outruns it.
#[derive(Default)]
pub struct Flow {
    paused: Mutex<bool>,
    resumed: Condvar,
}

/// A pause that outlives this is taken for a lost `pty_resume` (a webview that
/// reloaded mid-burst, an IPC call that failed) and read through. At worst that
/// costs one unpaced burst; a terminal frozen until it is restarted would be
/// worse. Far above the time xterm needs to parse a high-water mark's worth.
const MAX_PAUSE: Duration = Duration::from_secs(10);

impl Flow {
    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
        if !paused {
            self.resumed.notify_all();
        }
    }

    /// Block the reader while paused, for at most `MAX_PAUSE`.
    fn wait_until_resumed(&self) {
        self.wait_at_most(MAX_PAUSE);
    }

    fn wait_at_most(&self, max: Duration) {
        let deadline = Instant::now() + max;
        let mut paused = self.paused.lock().unwrap();
        while *paused {
            let now = Instant::now();
            if now >= deadline {
                // Cleared, not just skipped: left set, the next read would
                // wait another full `MAX_PAUSE`, and the terminal would crawl.
                *paused = false;
                break;
            }
            paused = self.resumed.wait_timeout(paused, deadline - now).unwrap().0;
        }
    }
}

pub struct SessionHandle {
    pub tx: mpsc::UnboundedSender<SessionCmd>,
    /// Milliseconds (since session start) of the last received output byte.
    /// Used by the startup engine's wait_for_prompt.
    pub last_output_ms: Arc<AtomicU64>,
    /// Resizing goes straight to the PTY master rather than through the
    /// command channel, so it still works while the writer is busy.
    resize: Arc<dyn Fn(u32, u32) + Send + Sync>,
    /// Kept so the foreground process group can be inspected (busy indicator,
    /// and the working directory a new terminal inherits). Unread on Windows,
    /// where a ConPTY has no foreground group and both answers come from the
    /// process tree instead — see `modules::process`.
    #[cfg_attr(windows, allow(dead_code))]
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    /// The shell's own pid, so agent detection can walk its descendants.
    pub shell_pid: Option<u32>,
    /// Whether the fallback busy poller should watch this session. Read by the
    /// Windows poller, which is app-global and so has to be told; the unix
    /// poller is per-session and captures the answer when it is spawned.
    #[cfg_attr(not(windows), allow(dead_code))]
    poll_busy: bool,
    instance: u64,
    /// Paused and resumed by `pty_pause` / `pty_resume`.
    flow: Arc<Flow>,
}

#[derive(Default)]
pub struct PtyManager(pub Mutex<HashMap<String, SessionHandle>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    status: String,
    message: Option<String>,
}

fn emit_status(app: &AppHandle, terminal_id: &str, status: &str, message: Option<String>) {
    app.emit(
        &format!("pty:status:{}", terminal_id),
        StatusPayload { status: status.into(), message },
    )
    .ok();
}

/// The user's login shell, or a sane platform default.
pub fn default_shell() -> String {
    if cfg!(windows) {
        return std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into());
    }
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

/// Shells available on this machine, for the settings dropdown. The login shell
/// is always first so the UI can present it as the default.
#[tauri::command]
pub fn shells_list() -> Vec<String> {
    if cfg!(windows) {
        return vec![
            "powershell.exe".into(),
            "pwsh.exe".into(),
            "cmd.exe".into(),
        ];
    }

    let login = default_shell();
    let mut shells = vec![login.clone()];

    // /etc/shells is the canonical list of valid login shells on unix.
    if let Ok(contents) = std::fs::read_to_string("/etc/shells") {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Only offer shells that actually exist, and never list one twice.
            if !shells.contains(&line.to_string()) && std::path::Path::new(line).exists() {
                shells.push(line.to_string());
            }
        }
    }
    shells
}

/// Quote a path for the shell that is about to re-parse the line we type into
/// it. POSIX quoting is wrong on Windows twice over: `shell_words::quote` quotes
/// *every* Windows path, since a backslash is special to a POSIX shell, and
/// cmd.exe — the default shell there — treats the single quotes it produces as
/// ordinary characters, so ssh receives them as part of the filename and reports
/// a key that exists as `Identity file 'C:\…\id_rsa' not accessible`.
///
/// A Windows path cannot contain `"`, and only a space makes one need quoting at
/// all. Leaving the common case bare is also what keeps it clear of PowerShell,
/// which expands `$` and backticks inside double quotes.
fn quote_key_path(path: &str, windows_shell: bool) -> String {
    if !windows_shell {
        return shell_words::quote(path).into_owned();
    }
    if path.contains(' ') {
        format!("\"{}\"", path)
    } else {
        path.to_string()
    }
}

/// Build the `ssh` argv for a saved connection, as a shell-ready command line.
/// Authenticating is deliberately left to OpenSSH: it reads ~/.ssh/config,
/// talks to ssh-agent, and verifies known_hosts, all of which this app would
/// otherwise have to reimplement worse.
///
/// `windows_shell` selects the quoting, and is a parameter rather than a `cfg!`
/// so that the tests cover both forms on either platform.
fn build_ssh_command(conn: &Connection, windows_shell: bool) -> String {
    let mut parts = vec!["ssh".to_string()];

    if conn.port != 22 {
        parts.push("-p".into());
        parts.push(conn.port.to_string());
    }
    if conn.auth_method == AuthMethod::Key {
        if let Some(key_path) = conn.key_path.as_deref().filter(|p| !p.is_empty()) {
            parts.push("-i".into());
            parts.push(quote_key_path(key_path, windows_shell));
        }
    }
    // The destination is only safe to type because `check_ssh_target` has kept
    // every shell-special character out of it. A backslash (`DOMAIN\user`) is
    // the one it allows that a POSIX shell would eat, hence the quote there;
    // `shell_words` leaves a plain `user@host` bare. On Windows the backslash is
    // literal in cmd and PowerShell alike, and nothing else needs quoting.
    let destination = format!("{}@{}", conn.username, conn.host);
    if windows_shell {
        parts.push(destination);
    } else {
        parts.push(shell_words::quote(&destination).into_owned());
    }
    parts.join(" ")
}

#[tauri::command]
pub async fn pty_open(
    app: AppHandle,
    terminal_id: String,
    // When set, an `ssh …` line for this connection is typed into the shell.
    connection_id: Option<String>,
    workspace_id: Option<String>,
    // Per-terminal shell override; falls back to the user's login shell.
    shell: Option<String>,
    // Directory to start the shell in; falls back to the user's home.
    cwd: Option<String>,
    cols: u32,
    rows: u32,
    // Where this session's output goes. One per `pty_open`, so a reconnect
    // never interleaves the old session's last bytes into the new stream.
    on_data: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let (conn, startup_tasks, vars, integration_enabled) = {
        let db = app.state::<Db>();
        let conn = match connection_id.as_deref() {
            Some(id) => Some(storage::get_connection(&db, id)?),
            None => None,
        };
        let startup_tasks: Vec<StartupTask> = storage::get_terminal(&db, &terminal_id)
            .map(|t| t.startup_tasks)
            .unwrap_or_default();
        let vars = variables::resolve(&db, workspace_id.as_deref(), Some(&terminal_id));
        // Opt-out, not opt-in: the integration writes nothing the user owns, so
        // the only reason to refuse it is a shell setup it upsets.
        let integration_enabled = storage::get_setting(&db, "terminal")
            .and_then(|v| v.get("shellIntegration").and_then(|b| b.as_bool()))
            .unwrap_or(true);
        (conn, startup_tasks, vars, integration_enabled)
    };

    // `connections_save` checks every connection it stores, but rows saved
    // before that check existed, or edited in the database by hand, never went
    // through it. Refuse here too, before a shell exists to type into.
    if let Some(c) = &conn {
        c.check_ssh_target().map_err(|e| {
            format!("Connection \"{}\" was not opened. {e} Edit the connection to fix it.", c.name)
        })?;
    }

    // Replace any existing session for this terminal (reconnect).
    close_session(&app, &terminal_id);
    emit_status(&app, &terminal_id, "connecting", None);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(1) as u16,
            cols: cols.max(1) as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Could not allocate a PTY: {}", e))?;

    let shell_path = shell.filter(|s| !s.is_empty()).unwrap_or_else(default_shell);
    let mut cmd = CommandBuilder::new(&shell_path);
    // Shell integration also owns the login-shell flag, since bash cannot take
    // both -l and --init-file. `integrated` false means the shell is unsupported
    // or the user turned it off, and the foreground-process poll fills in.
    let integrated = shell_integration::apply(&mut cmd, &shell_path, integration_enabled);
    // Start where the caller asked (a new terminal inherits the previous one's
    // directory), falling back to home if that path is gone.
    let start_dir = cwd
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(dirs::home_dir);
    if let Some(dir) = start_dir {
        cmd.cwd(dir);
    }
    cmd.env("TERM", "xterm-256color");
    // Identity for anything running inside this terminal that wants to report
    // back — Claude Code's hooks above all. Hook commands are grandchildren of
    // this shell, so they inherit these and can name the exact tab they belong
    // to; guessing from the working directory would be ambiguous the moment two
    // tabs sit in the same repository.
    cmd.env("KELYFOS_TERMINAL_ID", &terminal_id);
    if let Some(ws) = workspace_id.as_deref() {
        cmd.env("KELYFOS_WORKSPACE_ID", ws);
    }
    if let Some(sock) = crate::modules::agent::socket_path() {
        cmd.env("KELYFOS_HOOK_SOCKET", sock);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Could not start shell {}: {}", shell_path, e))?;
    // The shell's own pid: while the terminal's foreground process group equals
    // it, the shell sits at its prompt; anything else means a command is running.
    let shell_pid = child.process_id();
    // The slave fd must be dropped or the reader never sees EOF on exit.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Could not read from the PTY: {}", e))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Could not write to the PTY: {}", e))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<SessionCmd>();
    let last_output_ms = Arc::new(AtomicU64::new(0));
    let started = std::time::Instant::now();

    let master = Arc::new(Mutex::new(pair.master));
    let resize_master = master.clone();
    let resize = Arc::new(move |c: u32, r: u32| {
        if let Ok(m) = resize_master.lock() {
            m.resize(PtySize {
                rows: r.max(1) as u16,
                cols: c.max(1) as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok();
        }
    });

    // Fallback busy indicator, for shells with no integration (fish, every
    // Windows shell, or a user who turned it off). Skipped over SSH: the
    // foreground process is the local `ssh` client for the whole session, so it
    // would report busy forever while telling us nothing about the remote side.
    let poll_busy = connection_id.is_none() && !integrated;

    let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let flow = Arc::new(Flow::default());
    {
        let manager = app.state::<PtyManager>();
        manager.0.lock().unwrap().insert(
            terminal_id.clone(),
            SessionHandle {
                tx: tx.clone(),
                last_output_ms: last_output_ms.clone(),
                resize: resize.clone(),
                master: master.clone(),
                shell_pid,
                poll_busy,
                instance,
                flow: flow.clone(),
            },
        );
    }

    emit_status(&app, &terminal_id, "connected", None);

    // On unix the poll is one ioctl against this session's own PTY, so it stays
    // a per-session task and needs nothing shared. Windows has no foreground
    // group to ask and every tick costs a reading of the whole process table,
    // so N terminals would mean N readings per tick; there the poller is
    // app-global and started once — see `start_busy_poller`.
    #[cfg(unix)]
    if poll_busy {
        if let Some(shell_pid) = shell_pid {
            let app_poll = app.clone();
            let tid_poll = terminal_id.clone();
            let poll_master = master.clone();
            tauri::async_runtime::spawn(async move {
                let mut busy = false;
                let mut last_fg: Option<i32> = None;
                let mut last_fg_interactive = false;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
                    // Stop once this session is no longer the installed one
                    // (closed, or replaced by a reconnect).
                    {
                        let manager = app_poll.state::<PtyManager>();
                        let map = manager.0.lock().unwrap();
                        match map.get(&tid_poll) {
                            Some(s) if s.instance == instance => {}
                            _ => break,
                        }
                    }
                    let fg = poll_master
                        .lock()
                        .ok()
                        .and_then(|m| m.process_group_leader());
                    // A foreground group other than the shell's means something
                    // is running — but a long-lived interactive program is not
                    // work in progress, and reporting claude or vim as "busy"
                    // for as long as they are open makes the indicator useless.
                    // The name lookup only runs when the pid changes, since it
                    // costs a process spawn.
                    let now_busy = match fg {
                        Some(pid) if pid as u32 != shell_pid => {
                            if last_fg != Some(pid) {
                                last_fg = Some(pid);
                                last_fg_interactive = process_name(pid)
                                    .is_some_and(|n| process::is_interactive_program(&n));
                            }
                            !last_fg_interactive
                        }
                        _ => {
                            last_fg = None;
                            last_fg_interactive = false;
                            false
                        }
                    };
                    if now_busy != busy {
                        busy = now_busy;
                        let status = if busy { "busy" } else { "connected" };
                        emit_status(&app_poll, &tid_poll, status, None);
                    }
                }
            });
        }
    }

    // Reader: blocking reads off the PTY, so it gets a dedicated OS thread.
    let app_reader = app.clone();
    let tid_reader = terminal_id.clone();
    let last_output_reader = last_output_ms.clone();
    let tx_reader = tx.clone();
    let is_remote = connection_id.is_some();
    let flow_reader = flow.clone();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; READ_CHUNK];
        let mut scanner = MarkScanner::default();
        // A failed command holds the error status until the next command starts
        // or the user looks at the tab; without the latch the prompt mark that
        // follows immediately would clear the dot before it was ever seen.
        let mut failed = false;
        let mut saw_command = false;
        loop {
            flow_reader.wait_until_resumed();
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break, // shell exited or the PTY closed
                Ok(n) => {
                    last_output_reader
                        .store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                    // Fails only once the webview end of the channel is gone (a
                    // reload); the session is torn down from that side.
                    on_data.send(InvokeResponseBody::Raw(buf[..n].to_vec())).ok();

                    if !integrated {
                        continue;
                    }
                    for mark in scanner.feed(&buf[..n]) {
                        match mark {
                            // Over SSH the one command is the whole session, so
                            // its start says only "you are connected" — but its
                            // exit status is still worth having.
                            Mark::CommandStart => {
                                saw_command = true;
                                failed = false;
                                if !is_remote {
                                    emit_status(&app_reader, &tid_reader, "busy", None);
                                }
                            }
                            Mark::CommandEnd(code) => {
                                // The first prompt reports the status of nothing
                                // at all; acting on it would flash an error at
                                // startup for no reason.
                                if !saw_command {
                                    continue;
                                }
                                match code {
                                    Some(c) if marks::is_failure(c) => {
                                        failed = true;
                                        emit_status(
                                            &app_reader,
                                            &tid_reader,
                                            "error",
                                            Some(format!("Exited with code {}", c)),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                            Mark::Prompt => {
                                if !failed {
                                    emit_status(&app_reader, &tid_reader, "connected", None);
                                }
                            }
                        }
                    }
                }
            }
        }
        // EOF means the user exited the shell. Nothing else would ever wake the
        // writer task, so tell it to run teardown.
        tx_reader.send(SessionCmd::Close).ok();
    });

    // Writer: input and resizes arrive on the command channel.
    let app_writer = app.clone();
    let tid_writer = terminal_id.clone();
    let flow_writer = flow;
    tauri::async_runtime::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                SessionCmd::Data(bytes) => {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                    writer.flush().ok();
                }
                SessionCmd::Resize(c, r) => resize(c, r),
                SessionCmd::Close => break,
            }
        }
        // Killing the child drops the master, which unblocks the reader thread.
        child.kill().ok();
        child.wait().ok();
        // A reader parked by a pause is not in `read` and would never see the
        // EOF; release it so the thread can exit.
        flow_writer.set_paused(false);
        // Silent when a reconnect already replaced us — otherwise this stale
        // teardown would mark the live session disconnected.
        if remove_session(&app_writer, &tid_writer, instance) {
            emit_status(&app_writer, &tid_writer, "disconnected", None);
        }
    });

    // Type the connection's ssh line, then run any startup tasks. Both are
    // just bytes into the same shell, so they compose the way they would if
    // the user typed them.
    if conn.is_some() || !startup_tasks.is_empty() {
        // Always wait for the shell to draw its prompt first. Typed earlier, a
        // line is echoed by the tty and then echoed again by the shell's line
        // editor once it takes over — the command appears twice.
        let mut tasks = vec![StartupTask::WaitForPrompt];
        if let Some(conn) = conn {
            tasks.push(StartupTask::Command {
                value: build_ssh_command(&conn, cfg!(windows)),
            });
        }
        tasks.extend(startup_tasks);
        crate::modules::startup::run(tasks, vars, tx, last_output_ms);
    }

    Ok(())
}

fn close_session(app: &AppHandle, terminal_id: &str) {
    let manager = app.state::<PtyManager>();
    let existing = manager.0.lock().unwrap().remove(terminal_id);
    if let Some(s) = existing {
        s.tx.send(SessionCmd::Close).ok();
    }
}

/// Remove the map entry only if it is still the one this session installed;
/// a reconnect may already have replaced it. Returns whether it was still ours.
fn remove_session(app: &AppHandle, terminal_id: &str, instance: u64) -> bool {
    let manager = app.state::<PtyManager>();
    let mut map = manager.0.lock().unwrap();
    if map.get(terminal_id).is_some_and(|s| s.instance == instance) {
        map.remove(terminal_id);
        return true;
    }
    false
}

#[tauri::command]
pub fn pty_write(app: AppHandle, terminal_id: String, data: String) -> Result<(), String> {
    let manager = app.state::<PtyManager>();
    let map = manager.0.lock().unwrap();
    let session = map.get(&terminal_id).ok_or("no session")?;
    session
        .tx
        .send(SessionCmd::Data(data.into_bytes()))
        .map_err(|_| "session closed".to_string())
}

#[tauri::command]
pub fn pty_resize(app: AppHandle, terminal_id: String, cols: u32, rows: u32) -> Result<(), String> {
    let manager = app.state::<PtyManager>();
    let map = manager.0.lock().unwrap();
    let session = map.get(&terminal_id).ok_or("no session")?;
    (session.resize)(cols, rows);
    Ok(())
}

#[tauri::command]
pub fn pty_close(app: AppHandle, terminal_id: String) {
    close_session(&app, &terminal_id);
}

/// Flow control from the frontend; see `Flow`. A terminal with no session
/// (closed, or not open yet) has nothing to pace, so neither is an error.
#[tauri::command]
pub fn pty_pause(app: AppHandle, terminal_id: String) {
    set_paused(&app, &terminal_id, true);
}

#[tauri::command]
pub fn pty_resume(app: AppHandle, terminal_id: String) {
    set_paused(&app, &terminal_id, false);
}

fn set_paused(app: &AppHandle, terminal_id: &str, paused: bool) {
    // Clone the handle out first: the reader holds the flow's own lock while it
    // waits, and the session map is not something to hold across that.
    let flow = {
        let manager = app.state::<PtyManager>();
        let map = manager.0.lock().unwrap();
        map.get(terminal_id).map(|s| s.flow.clone())
    };
    if let Some(flow) = flow {
        flow.set_paused(paused);
    }
}

/// Start the Windows fallback busy poller. One task for the whole app rather
/// than one per session: every tick needs a reading of the process table, and
/// per-session tasks would each want their own even with the short-TTL cache
/// absorbing most of the cost. It iterates the open sessions the way
/// `agent::poll_once` does — take the list under the `PtyManager` lock, release
/// it, then evaluate.
///
/// This is the *only* busy signal Windows will ever have: `shell_integration`
/// maps every Windows shell to `Unsupported`, so there is no OSC 133 underneath
/// it. Its accuracy is user-facing, not a fallback nobody sees.
#[cfg(windows)]
pub fn start_busy_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut seen: HashMap<String, BusyState> = HashMap::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
            busy_tick(&app, &mut seen);
        }
    });
}

/// What the Windows poller remembers about one terminal between ticks.
#[cfg(windows)]
#[derive(Default)]
struct BusyState {
    /// Which session this belongs to; a reconnect resets the rest.
    instance: u64,
    /// The dot currently being shown.
    busy: bool,
    /// A command was seen last tick. The dot waits for two in a row.
    candidate: bool,
    /// `last_output_ms` when this terminal was last looked at.
    output_at: u64,
}

#[cfg(windows)]
fn busy_tick(app: &AppHandle, seen: &mut HashMap<String, BusyState>) {
    // (terminal id, instance, shell pid, ms of the last byte received)
    let sessions: Vec<(String, u64, u32, u64)> = {
        let manager = app.state::<PtyManager>();
        let map = manager.0.lock().unwrap();
        map.iter()
            .filter(|(_, s)| s.poll_busy)
            .filter_map(|(id, s)| {
                s.shell_pid.map(|pid| {
                    (
                        id.clone(),
                        s.instance,
                        pid,
                        s.last_output_ms.load(Ordering::Relaxed),
                    )
                })
            })
            .collect()
    };
    // Forget terminals the user closed.
    seen.retain(|id, _| sessions.iter().any(|(open, _, _, _)| open == id));

    // Reading the process table is by far the most expensive thing this app does
    // on a timer — about 7 ms on a machine with 500 processes, and no documented
    // Windows API is cheaper (see modules::process). At 350 ms that would be
    // ~2% of a core burned forever by an app with a terminal sitting at an idle
    // prompt, which is not a price a status dot is worth.
    //
    // So the table is only read when something could actually have changed:
    //
    //   * the terminal produced output since the last look. A command cannot
    //     start without the shell at least echoing the line that started it, and
    //     it cannot end without a prompt being drawn.
    //   * or a command was already running last tick. `Start-Sleep 5` is silent
    //     for its whole run, so output alone would never see it finish — and
    //     without this the two-tick settle below could never reach its second
    //     tick either.
    //   * or the terminal is new and has never been looked at.
    //
    // An idle terminal therefore costs one atomic load per tick, and the poll
    // wakes up properly the moment the user does anything.
    let worth_reading = sessions.iter().any(|(id, instance, _, output_at)| {
        match seen.get(id) {
            Some(state) if state.instance == *instance => {
                state.candidate || state.output_at != *output_at
            }
            _ => true,
        }
    });
    if !worth_reading {
        return;
    }

    let Some(tree) = process::ProcessTree::snapshot() else {
        return;
    };
    // Enumerating windows is cheap (tens of microseconds) but can only ever take
    // candidates away, so it waits until something looks busy.
    let mut windowed: Option<std::collections::HashSet<u32>> = None;

    for (terminal_id, instance, shell_pid, output_at) in sessions {
        let candidate = process::running_command_pid(&tree, shell_pid, &|_| false).is_some() && {
            let w = windowed.get_or_insert_with(process::windowed_pids);
            process::running_command_pid(&tree, shell_pid, &|pid| w.contains(&pid)).is_some()
        };

        let state = seen.entry(terminal_id.clone()).or_default();
        // A reconnect replaces a session while the old one is still tearing
        // down. The new one starts from a clean slate rather than inheriting a
        // dot that belonged to its predecessor.
        if state.instance != instance {
            *state = BusyState { instance, ..Default::default() };
        }
        state.output_at = output_at;

        // Report busy only once a command has survived two consecutive ticks.
        // That hides the short-lived helpers a shell spawns for its own prompt,
        // and gives a GUI app the moment it needs to put a window on screen
        // before the filter above can recognise it as detached. Clearing is
        // immediate: the stale busy dot is the one that annoys.
        let busy = candidate && state.candidate;
        state.candidate = candidate;
        if busy != state.busy {
            state.busy = busy;
            let status = if busy { "busy" } else { "connected" };
            emit_status(app, &terminal_id, status, None);
        }
    }
}

/// Working directory of a terminal's foreground process, so a new terminal can
/// open where the user currently is. None when it cannot be determined (no
/// session, or an unsupported platform); the caller then falls back to home.
/// For an SSH terminal this is the local `ssh` client's directory — the remote
/// one is not visible from here.
#[tauri::command]
pub async fn pty_cwd(app: AppHandle, terminal_id: String) -> Option<String> {
    // Scoped so the (non-Send) lock guard never crosses an await point.
    let pid = {
        let manager = app.state::<PtyManager>();
        let map = manager.0.lock().unwrap();
        foreground_pid(map.get(&terminal_id)?)?
    };
    cwd_of_pid(pid)
}

/// The process whose directory a new terminal should inherit.
///
/// The terminal knows this exactly on unix: the foreground process group leader
/// is whatever the user is looking at, and the shell itself when they are at the
/// prompt.
#[cfg(unix)]
fn foreground_pid(session: &SessionHandle) -> Option<i32> {
    session.master.lock().ok()?.process_group_leader()
}

/// Windows has no foreground group, so the nearest honest answer is the deepest
/// command running under the shell — and the shell itself when nothing is, which
/// is the common case here: the user presses "new terminal" at a prompt, not in
/// the middle of a build.
#[cfg(windows)]
fn foreground_pid(session: &SessionHandle) -> Option<i32> {
    let shell_pid = session.shell_pid?;
    let running = process::ProcessTree::snapshot().and_then(|tree| {
        let windowed = process::windowed_pids();
        process::running_command_pid(&tree, shell_pid, &|pid| windowed.contains(&pid))
    });
    Some(running.unwrap_or(shell_pid) as i32)
}

/// Executable name of a process, normalised, or None where it cannot be read.
/// Only the unix poller asks: Windows classifies from the process tree, which
/// carries names already.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn process_name(pid: i32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let raw = String::from_utf8_lossy(&out.stdout);
    let name = raw.trim();
    if name.is_empty() {
        return None;
    }
    // The same normaliser the process tree uses, so the one interactive-program
    // list cannot mean two different things depending on who asked.
    Some(process::normalize_name(name))
}

#[cfg(target_os = "linux")]
fn cwd_of_pid(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{}/cwd", pid))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// macOS has no /proc; `lsof` is the standard way to read a process's cwd.
/// `-Fn` prints one field per line, the path prefixed with `n`.
#[cfg(target_os = "macos")]
fn cwd_of_pid(pid: i32) -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-d", "cwd", "-Fn", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('n'))
        .map(str::to_string)
}

#[cfg(windows)]
fn cwd_of_pid(pid: i32) -> Option<String> {
    process::cwd_of_pid(pid as u32)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn cwd_of_pid(_pid: i32) -> Option<String> {
    None
}

/// Nothing to inherit on a platform with no way to read a process's directory.
#[cfg(not(any(unix, windows)))]
fn foreground_pid(_session: &SessionHandle) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_does_not_block_when_not_paused() {
        let flow = Flow::default();
        let start = Instant::now();
        flow.wait_at_most(Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn flow_resume_releases_a_waiting_reader() {
        let flow = Arc::new(Flow::default());
        flow.set_paused(true);
        let resumer = flow.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            resumer.set_paused(false);
        });
        let start = Instant::now();
        flow.wait_at_most(Duration::from_secs(5));
        let waited = start.elapsed();
        handle.join().unwrap();
        assert!(waited >= Duration::from_millis(40), "returned early: {waited:?}");
        assert!(waited < Duration::from_secs(2), "missed the resume: {waited:?}");
    }

    /// A lost resume must not freeze the terminal: the wait ends on its own and
    /// clears the flag, so the next read does not wait all over again.
    #[test]
    fn flow_reads_through_a_pause_nobody_resumes() {
        let flow = Flow::default();
        flow.set_paused(true);
        flow.wait_at_most(Duration::from_millis(30));
        let start = Instant::now();
        flow.wait_at_most(Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    fn conn(auth: AuthMethod, port: u16, key_path: Option<&str>) -> Connection {
        Connection {
            id: "c1".into(),
            name: "test".into(),
            host: "example.com".into(),
            port,
            username: "root".into(),
            description: String::new(),
            icon: String::new(),
            color: String::new(),
            tags: vec![],
            folder: String::new(),
            auth_method: auth,
            key_path: key_path.map(String::from),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn omits_default_port() {
        let c = conn(AuthMethod::Password, 22, None);
        assert_eq!(build_ssh_command(&c, false), "ssh root@example.com");
    }

    #[test]
    fn includes_non_default_port() {
        let c = conn(AuthMethod::Password, 2222, None);
        assert_eq!(build_ssh_command(&c, false), "ssh -p 2222 root@example.com");
    }

    #[test]
    fn includes_identity_file_for_key_auth() {
        let c = conn(AuthMethod::Key, 22, Some("/home/u/.ssh/id_ed25519"));
        assert_eq!(
            build_ssh_command(&c, false),
            "ssh -i /home/u/.ssh/id_ed25519 root@example.com"
        );
    }

    #[test]
    fn ignores_key_path_when_auth_is_password() {
        let c = conn(AuthMethod::Password, 22, Some("/home/u/.ssh/id_ed25519"));
        assert_eq!(build_ssh_command(&c, false), "ssh root@example.com");
    }

    /// A key path with a space must not split into two argv entries once the
    /// shell re-parses the line we type into it.
    #[test]
    fn quotes_key_path_with_spaces() {
        let c = conn(AuthMethod::Key, 22, Some("/home/u/my keys/id_ed25519"));
        assert_eq!(
            build_ssh_command(&c, false),
            "ssh -i '/home/u/my keys/id_ed25519' root@example.com"
        );
    }

    #[test]
    fn treats_empty_key_path_as_absent() {
        let c = conn(AuthMethod::Key, 22, Some(""));
        assert_eq!(build_ssh_command(&c, false), "ssh root@example.com");
    }

    /// The regression: POSIX quoting of a Windows path reaches ssh with the
    /// quotes attached, and cmd.exe never strips them.
    #[test]
    fn leaves_windows_key_path_unquoted() {
        let c = conn(AuthMethod::Key, 22, Some(r"C:\Users\u\.ssh\id_rsa"));
        assert_eq!(
            build_ssh_command(&c, true),
            r"ssh -i C:\Users\u\.ssh\id_rsa root@example.com"
        );
    }

    /// `DOMAIN\user` reaches ssh intact: a POSIX shell would drop the backslash
    /// unquoted, cmd and PowerShell keep it as typed.
    #[test]
    fn quotes_backslash_username_only_for_posix() {
        let mut c = conn(AuthMethod::Password, 22, None);
        c.username = r"CORP\alice".into();
        assert_eq!(build_ssh_command(&c, false), r"ssh 'CORP\alice@example.com'");
        assert_eq!(build_ssh_command(&c, true), r"ssh CORP\alice@example.com");
    }

    /// A space still needs quoting on Windows, in the one form cmd, powershell
    /// and pwsh all read the same way.
    #[test]
    fn double_quotes_windows_key_path_with_spaces() {
        let c = conn(AuthMethod::Key, 22, Some(r"C:\my keys\id_rsa"));
        assert_eq!(
            build_ssh_command(&c, true),
            "ssh -i \"C:\\my keys\\id_rsa\" root@example.com"
        );
    }
}
