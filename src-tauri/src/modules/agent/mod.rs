//! Claude Code awareness: which terminals are running it, and what it is doing.
//!
//! Two independent signals, deliberately layered so the app is useful to people
//! who never opt in and to people who do not have Claude Code at all:
//!
//!   * presence — a `claude` process among a terminal's descendants. Needs no
//!     configuration, and is what distinguishes "Claude is open" from "Claude is
//!     working". It also reins in the hook state: a session killed outright
//!     never sends its closing event, and without this the tab would blink for
//!     the rest of the day.
//!   * hooks — exact lifecycle events, once the user connects. Only these can
//!     tell working apart from waiting-for-you, which is invisible from outside
//!     the process.
//!
//! Everything here degrades to "no Claude" quietly. Nothing in this module is
//! required for a terminal to work.

pub mod hooks;
pub mod install;
pub mod stats;

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::modules::process::{self, ProcessTree};
use crate::modules::pty::PtyManager;

pub use hooks::socket_path;

/// How long a running or waiting state may go without any event before the
/// watchdog distrusts it. Long enough that a slow tool call is never cut short.
const STALE_MS: u64 = 90_000;

/// Presence polling interval. One reading of the process table covers every
/// terminal, and it is shared with the Windows busy poller.
const POLL_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    #[default]
    Idle,
    Running,
    Waiting,
}

#[derive(Debug, Default)]
pub struct AgentState {
    /// A claude process is running in this terminal.
    pub present: bool,
    pub phase: AgentPhase,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    /// Tool currently running, for the tooltip.
    pub tool: Option<String>,
    /// Open subagent calls; their Stop must not end the whole turn.
    pub depth: i32,
    pub last_tool_error: bool,
    updated_at: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl AgentState {
    pub fn touch(&mut self) {
        self.updated_at = now_ms();
    }

    pub fn snapshot(&self, terminal_id: &str) -> AgentSnapshot {
        AgentSnapshot {
            terminal_id: terminal_id.to_string(),
            present: self.present,
            phase: self.phase,
            session_id: self.session_id.clone(),
            transcript_path: self.transcript_path.clone(),
            cwd: self.cwd.clone(),
            tool: self.tool.clone(),
            last_tool_error: self.last_tool_error,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub terminal_id: String,
    pub present: bool,
    pub phase: AgentPhase,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub tool: Option<String>,
    pub last_tool_error: bool,
}

#[derive(Default)]
pub struct AgentManager(pub Mutex<HashMap<String, AgentState>>);

/// When a hook event last arrived, and when the self-test's did. Statics rather
/// than managed state because they are read from a plain command with no window
/// involved, and there is only ever one app.
static LAST_EVENT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_TEST_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn record_event() {
    LAST_EVENT_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

pub fn record_test_event() {
    LAST_TEST_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

pub fn emit(app: &AppHandle, snapshot: AgentSnapshot) {
    app.emit("agent:state", snapshot).ok();
}

/// Start the hook listener and the presence poller. Safe to call when Claude
/// Code is not installed: the poller simply never finds anything.
pub fn start(app: AppHandle) {
    hooks::start(app.clone());

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
            poll_once(&app);
        }
    });
}

fn poll_once(app: &AppHandle) {
    // terminal id -> shell pid, for the sessions currently open.
    let sessions: Vec<(String, Option<u32>)> = {
        let manager = app.state::<PtyManager>();
        let map = manager.0.lock().unwrap();
        map.iter().map(|(id, s)| (id.clone(), s.shell_pid)).collect()
    };

    let tree = ProcessTree::snapshot();
    let mut changed: Vec<AgentSnapshot> = Vec::new();
    {
        let manager = app.state::<AgentManager>();
        let mut states = manager.0.lock().unwrap();

        // Forget terminals that are gone, so a long session does not accumulate
        // state for tabs the user closed hours ago.
        let open: std::collections::HashSet<&str> =
            sessions.iter().map(|(id, _)| id.as_str()).collect();
        states.retain(|id, _| open.contains(id.as_str()));

        for (terminal_id, shell_pid) in &sessions {
            let present = match (&tree, shell_pid) {
                // Not an exact name match: on Windows an npm-installed Claude
                // Code is a `node` process and there is no `claude` to find.
                (Some(tree), Some(pid)) => tree.has_descendant_matching(*pid, process::is_claude),
                // Without a process list, believe whatever the hooks last said
                // rather than wrongly declaring Claude gone.
                _ => states.get(terminal_id).is_some_and(|s| s.present),
            };

            let entry = states.entry(terminal_id.clone()).or_default();
            let was = (entry.present, entry.phase);

            entry.present = present;
            if !present {
                // The process is gone, so nothing is running whatever the last
                // event claimed. This is the case a missed Stop would otherwise
                // leave blinking forever.
                entry.phase = AgentPhase::Idle;
                entry.tool = None;
                entry.depth = 0;
            } else if entry.phase != AgentPhase::Idle && now_ms().saturating_sub(entry.updated_at) > STALE_MS {
                // Still running, but silent far longer than any hook gap should
                // be — most likely the events stopped reaching us.
                entry.phase = AgentPhase::Idle;
                entry.tool = None;
            }

            if was != (entry.present, entry.phase) {
                entry.touch();
                changed.push(entry.snapshot(terminal_id));
            }
        }
    }

    for snapshot in changed {
        emit(app, snapshot);
    }
}

// ---------- Commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHealth {
    /// Claude Code is on this machine at all. Everything else is moot without it.
    pub installed: bool,
    pub version: Option<String>,
    /// All expected hook events are installed and point at our shim.
    pub connected: bool,
    /// Some but not all — usually a hand-edited settings.json.
    pub partial: bool,
    pub shim_installed: bool,
    pub socket_listening: bool,
    pub settings_path: Option<String>,
    pub shim_path: Option<String>,
    /// When Claude last reported anything, as epoch ms. None means the hooks
    /// have never fired — which is the difference between "connected" and
    /// "connected and actually working" that the row of ticks cannot show.
    pub last_event_at: Option<u64>,
    /// Claude's own theme setting ("dark", "dark-ansi", "custom:kelyfos-theme",
    /// …). None when unset — Claude's default, which does not follow the
    /// terminal palette.
    pub theme: Option<String>,
    /// The Kelyfos theme file is present in ~/.claude/themes and current.
    /// Says nothing about being *selected* — that is `theme`.
    pub theme_installed: bool,
}

/// Whether the `claude` binary exists, and its version. Runs `claude --version`
/// with a short leash: a missing or hung binary must not stall the settings UI.
fn claude_version() -> Option<String> {
    let out = process::command("claude")
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[tauri::command]
pub async fn claude_health() -> ClaudeHealth {
    let version = claude_version();
    let installed = version.is_some() || install::claude_dir().is_some_and(|d| d.is_dir());
    let events = install::installed_events();
    ClaudeHealth {
        installed,
        version,
        connected: events == install::EXPECTED_EVENT_COUNT,
        partial: events > 0 && events < install::EXPECTED_EVENT_COUNT,
        shim_installed: install::shim_installed(),
        socket_listening: hooks::listening(),
        settings_path: install::settings_path().map(|p| p.to_string_lossy().into_owned()),
        shim_path: install::shim_path().map(|p| p.to_string_lossy().into_owned()),
        last_event_at: match LAST_EVENT_MS.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            ms => Some(ms),
        },
        theme: install::theme(),
        theme_installed: install::theme_installed(),
    }
}

/// Write (or refresh) the Kelyfos theme file for Claude. User-initiated only;
/// never selects it — the "theme" key in settings.json is not touched.
#[tauri::command]
pub async fn claude_theme_install() -> Result<(), String> {
    install::install_theme()
}

#[tauri::command]
pub async fn claude_theme_remove() -> Result<(), String> {
    install::remove_theme()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub message: String,
}

/// Run the hook script exactly as Claude would, and see whether it reaches us.
///
/// The row of ticks can only report that each piece exists; this is the one
/// check that proves the whole chain carries a message end to end, which is
/// what a user actually wants to know when the status dots stay dark.
#[tauri::command]
pub async fn claude_test() -> TestResult {
    let Some(shim) = install::shim_path().filter(|p| p.is_file()) else {
        return TestResult {
            ok: false,
            message: "The hook script is missing. Connect again to reinstall it.".into(),
        };
    };
    let Some(socket) = socket_path() else {
        return TestResult {
            ok: false,
            message: "Kelyfos has nowhere to open its hook endpoint — no data directory.".into(),
        };
    };
    if !hooks::listening() {
        return TestResult {
            ok: false,
            message: "Kelyfos is not listening for hook events. Restarting the app usually fixes this.".into(),
        };
    }

    let before = LAST_TEST_MS.load(std::sync::atomic::Ordering::Relaxed);
    let payload = r#"{"hook_event_name":"Notification","session_id":"kelyfos-self-test"}"#;

    // Run the shim the way Claude will, which means each platform's own
    // interpreter and flags — a test that took a shortcut here would pass on a
    // machine where the real hook cannot start.
    #[cfg(windows)]
    let mut command = {
        let mut c = process::command("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]);
        c
    };
    #[cfg(not(windows))]
    let mut command = process::command("/bin/sh");

    let spawned = command
        .arg(&shim)
        .env("KELYFOS_HOOK_SOCKET", &socket)
        .env("KELYFOS_TERMINAL_ID", hooks::TEST_TERMINAL_ID)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            return TestResult {
                ok: false,
                message: format!("The hook script could not be run: {e}"),
            }
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(payload.as_bytes()).ok();
    }
    child.wait().ok();

    // The script hands off to a background sender and returns at once, so the
    // message arrives shortly after it exits rather than before.
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if LAST_TEST_MS.load(std::sync::atomic::Ordering::Relaxed) > before {
            return TestResult {
                ok: true,
                message: "Hook delivered. Claude can reach Kelyfos.".into(),
            };
        }
    }

    TestResult {
        ok: false,
        message: "The hook script ran but nothing arrived. It needs either nc or python3 on your PATH to reach Kelyfos.".into(),
    }
}

#[tauri::command]
pub async fn claude_connect() -> Result<(), String> {
    install::install()
}

#[tauri::command]
pub async fn claude_disconnect() -> Result<(), String> {
    install::uninstall()
}

/// Current agent state for every open terminal, so a freshly opened window does
/// not have to wait for the next event to know what is going on.
#[tauri::command]
pub fn claude_states(app: AppHandle) -> Vec<AgentSnapshot> {
    let manager = app.state::<AgentManager>();
    let states = manager.0.lock().unwrap();
    states.iter().map(|(id, s)| s.snapshot(id)).collect()
}

// The process-tree tests moved to `modules::process` along with the type.
