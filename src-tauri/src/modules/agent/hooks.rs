//! Receiving hook events from Claude Code: a Unix socket on macOS and Linux, a
//! named pipe on Windows.
//!
//! Only the transport differs. Both endpoints are reachable by anything running
//! as this user, so every payload is treated as untrusted input: it is parsed,
//! its terminal id is checked against the sessions actually open, and nothing in
//! it is ever executed. The worst a forged message can do is light up the wrong
//! status dot. `handle` below is the single reader of both, which is what keeps
//! that guarantee from having two versions.

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use super::{AgentManager, AgentPhase};
use crate::modules::pty::PtyManager;

/// A hook payload larger than this is not one of Claude's; reading further
/// would let a stray writer hold the task open indefinitely.
const MAX_MESSAGE: usize = 256 * 1024;

/// Terminal id the self-test sends under. It names no real terminal, so it is
/// recognised before the check that drops unknown ones and never becomes agent
/// state — its only job is to prove the shim can reach the socket.
pub const TEST_TERMINAL_ID: &str = "__kelyfos_selftest__";

#[derive(Deserialize)]
struct Message {
    #[serde(rename = "terminalId")]
    terminal_id: String,
    event: Event,
}

/// Only the fields we act on. Claude sends more, and may send more still in a
/// future version; unknown ones are ignored rather than treated as an error.
#[derive(Deserialize)]
struct Event {
    #[serde(rename = "hook_event_name")]
    name: String,
    #[serde(rename = "session_id")]
    session_id: Option<String>,
    #[serde(rename = "transcript_path")]
    transcript_path: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "tool_name")]
    tool_name: Option<String>,
    #[serde(rename = "tool_response")]
    tool_response: Option<serde_json::Value>,
}

/// Where the shim sends events. A filesystem path on unix, a pipe name on
/// Windows — the shim only ever passes it back to the API that opens it, so one
/// environment variable carries either.
#[cfg(unix)]
pub fn socket_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("kelyfos").join("hooks.sock"))
}

#[cfg(windows)]
pub fn socket_path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(pipe_name()))
}

#[cfg(not(any(unix, windows)))]
pub fn socket_path() -> Option<std::path::PathBuf> {
    None
}

/// Pipe names live in one namespace for the whole machine, unlike a socket file
/// under the user's own data directory. Two people signed in at once — fast user
/// switching, or a terminal server — would otherwise share a name, and each
/// hook would go to whichever app happened to be waiting. Suffixing the user
/// name keeps them apart; characters that are not legal in a pipe name are
/// dropped rather than escaped, since this only has to be distinct, not
/// reversible.
#[cfg(windows)]
fn pipe_name() -> String {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if user.is_empty() {
        r"\\.\pipe\kelyfos-hooks".into()
    } else {
        format!(r"\\.\pipe\kelyfos-hooks-{user}")
    }
}

/// Whether the endpoint is actually open, which is the only part of the chain
/// the diagnostics panel cannot see from the filesystem on both platforms: a
/// socket leaves a file behind, a pipe leaves nothing to stat.
#[cfg(unix)]
pub fn listening() -> bool {
    socket_path().is_some_and(|p| p.exists())
}

#[cfg(windows)]
pub fn listening() -> bool {
    LISTENING.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(not(any(unix, windows)))]
pub fn listening() -> bool {
    false
}

#[cfg(windows)]
static LISTENING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
pub fn start(app: AppHandle) {
    let Some(path) = socket_path() else { return };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    // A socket file left behind by a crash would refuse the bind; the app is
    // the only writer of this path, so removing it is safe.
    std::fs::remove_file(&path).ok();

    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("kelyfos: could not open the Claude hook socket: {e}");
            return;
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    }

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let app = app.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut text = String::new();
                if stream
                    .take(MAX_MESSAGE as u64)
                    .read_to_string(&mut text)
                    .is_err()
                {
                    return;
                }
                handle(&app, &text);
            });
        }
    });
}

/// The same listener over a named pipe, which is the nearest Windows equivalent
/// of a Unix socket: local only, addressed by name rather than by port, and
/// openable by an ordinary child process without any networking.
///
/// The shape differs from the unix loop in one way that matters. A socket
/// listener accepts as many connections as arrive; a pipe server instance
/// handles exactly one client and is then spent, so a fresh instance has to be
/// waiting before the last is handed off. Claude fires hooks in bursts — a
/// PreToolUse and a PostToolUse around every tool call — and a client that
/// arrives while no instance is waiting is refused outright, leaving the dot
/// wrong until the next event happens to land.
#[cfg(windows)]
pub fn start(app: AppHandle) {
    use std::sync::atomic::Ordering;
    use tokio::io::AsyncReadExt;
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = pipe_name();
    tauri::async_runtime::spawn(async move {
        // first_pipe_instance claims the name. Without it a second Kelyfos for
        // this user would silently add its own server to the same pipe, and
        // events would go to whichever process happened to be waiting — the
        // failure a user could never diagnose. Failing the create instead means
        // the second window simply reports that it is not listening.
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("kelyfos: could not open the Claude hook pipe: {e}");
                return;
            }
        };
        LISTENING.store(true, Ordering::Relaxed);

        loop {
            let connected = server.connect().await;
            // Replaced whether the wait succeeded or not: either way this
            // instance is used up, and recreating here rather than after the
            // handoff is what keeps an instance permanently available. A create
            // that fails is not transient — the name is gone — so the loop ends
            // and the panel stops claiming to listen.
            let stream = match ServerOptions::new().create(&name) {
                Ok(next) => std::mem::replace(&mut server, next),
                Err(e) => {
                    eprintln!("kelyfos: the Claude hook pipe closed: {e}");
                    break;
                }
            };
            if connected.is_err() {
                continue;
            }

            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut buf = Vec::new();
                // A client that connects and then never writes would otherwise
                // hold this task for as long as it liked. The shim writes and
                // exits in one go, so anything still silent after five seconds
                // is not it.
                let read = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    stream.take(MAX_MESSAGE as u64).read_to_end(&mut buf),
                )
                .await;
                if read.is_err() {
                    return;
                }
                if let Ok(text) = std::str::from_utf8(&buf) {
                    handle(&app, text);
                }
            });
        }

        LISTENING.store(false, Ordering::Relaxed);
    });
}

#[cfg(not(any(unix, windows)))]
pub fn start(_app: AppHandle) {}

/// Parse one message and fold it into the terminal's agent state.
fn handle(app: &AppHandle, text: &str) {
    let Ok(msg) = serde_json::from_str::<Message>(text.trim()) else {
        return;
    };
    if msg.terminal_id.is_empty() {
        return;
    }
    if msg.terminal_id == TEST_TERMINAL_ID {
        super::record_test_event();
        return;
    }
    super::record_event();
    // Drop events naming a terminal that is not open: a stale hook from a shell
    // that outlived its tab, or something else on the machine talking to us.
    {
        let manager = app.state::<PtyManager>();
        let map = manager.0.lock().unwrap();
        if !map.contains_key(&msg.terminal_id) {
            return;
        }
    }

    let manager = app.state::<AgentManager>();
    let mut states = manager.0.lock().unwrap();
    let entry = states.entry(msg.terminal_id.clone()).or_default();
    let e = &msg.event;

    entry.present = true;
    if e.session_id.is_some() {
        entry.session_id = e.session_id.clone();
    }
    if e.transcript_path.is_some() {
        entry.transcript_path = e.transcript_path.clone();
    }
    if e.cwd.is_some() {
        entry.cwd = e.cwd.clone();
    }
    entry.touch();

    match e.name.as_str() {
        "SessionStart" => {
            entry.phase = AgentPhase::Idle;
            entry.depth = 0;
            entry.tool = None;
        }
        "UserPromptSubmit" => {
            entry.phase = AgentPhase::Running;
            entry.tool = None;
        }
        "PreToolUse" => {
            entry.phase = AgentPhase::Running;
            entry.tool = e.tool_name.clone();
            // A Task tool call is a subagent: its own Stop must not be mistaken
            // for the whole turn finishing.
            if e.tool_name.as_deref() == Some("Task") {
                entry.depth += 1;
            }
        }
        "PostToolUse" => {
            entry.phase = AgentPhase::Running;
            entry.tool = None;
            if tool_failed(e.tool_response.as_ref()) {
                entry.last_tool_error = true;
            }
        }
        // Claude wants the user: a permission prompt, or a question.
        "Notification" => entry.phase = AgentPhase::Waiting,
        "SubagentStop" => {
            entry.depth = (entry.depth - 1).max(0);
        }
        "Stop" => {
            if entry.depth == 0 {
                entry.phase = AgentPhase::Idle;
                entry.tool = None;
            }
        }
        "SessionEnd" => {
            entry.phase = AgentPhase::Idle;
            entry.present = false;
            entry.session_id = None;
            entry.depth = 0;
        }
        _ => {}
    }

    let snapshot = entry.snapshot(&msg.terminal_id);
    drop(states);
    super::emit(app, snapshot);
}

/// Claude reports tool failures inside the response rather than as an event of
/// their own, and the shape differs per tool — so this stays deliberately
/// shallow. It only feeds a tooltip; the error dot is reserved for a session
/// that actually died, which is the signal a user can act on.
fn tool_failed(response: Option<&serde_json::Value>) -> bool {
    let Some(v) = response else { return false };
    if let Some(b) = v.get("is_error").and_then(serde_json::Value::as_bool) {
        return b;
    }
    v.get("success")
        .and_then(serde_json::Value::as_bool)
        .map(|ok| !ok)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_wrapped_hook_payload() {
        let text = r#"{"terminalId":"t1","event":{"hook_event_name":"Notification","session_id":"s1","cwd":"/tmp"}}"#;
        let msg: Message = serde_json::from_str(text).unwrap();
        assert_eq!(msg.terminal_id, "t1");
        assert_eq!(msg.event.name, "Notification");
        assert_eq!(msg.event.session_id.as_deref(), Some("s1"));
    }

    /// Claude adds fields over time; an unknown one must not drop the event.
    #[test]
    fn ignores_unknown_fields() {
        let text = r#"{"terminalId":"t1","event":{"hook_event_name":"Stop","brand_new_field":42}}"#;
        assert!(serde_json::from_str::<Message>(text).is_ok());
    }

    #[test]
    fn rejects_junk() {
        assert!(serde_json::from_str::<Message>("not json").is_err());
        assert!(serde_json::from_str::<Message>("{}").is_err());
    }

    #[test]
    fn detects_tool_failure_shapes() {
        assert!(tool_failed(Some(&serde_json::json!({ "is_error": true }))));
        assert!(tool_failed(Some(&serde_json::json!({ "success": false }))));
        assert!(!tool_failed(Some(&serde_json::json!({ "success": true }))));
        assert!(!tool_failed(Some(&serde_json::json!({ "stdout": "ok" }))));
        assert!(!tool_failed(None));
    }
}
