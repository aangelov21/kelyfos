//! Startup task engine.
//! Runs a task list against a freshly connected terminal session.
//! Supported: wait_for_prompt, command, wait.
//! New task types are added as `StartupTask` enum variants plus a match arm here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::modules::models::{SessionCmd, StartupTask};
use crate::modules::variables;

/// How long the terminal must stay silent before we consider the prompt ready.
const PROMPT_IDLE_MS: u64 = 600;
/// Give up waiting for a prompt after this long.
const PROMPT_TIMEOUT_MS: u64 = 15_000;
/// Cap on the settling wait every command does for itself. Shorter than the
/// explicit task's, because this one is paid on every command in the list and
/// something that never falls silent (a progress bar, `top`) must not stall it
/// for fifteen seconds each time.
const SETTLE_TIMEOUT_MS: u64 = 5_000;

/// Block until the terminal has been quiet for `PROMPT_IDLE_MS`, or `timeout_ms`
/// passes.
///
/// `last > 0` matters: a session that has printed nothing at all has not
/// reached a prompt, it just started, and treating that silence as readiness is
/// how a command ends up typed into a shell that is not listening yet.
async fn settle(started: &std::time::Instant, last_output_ms: &Arc<AtomicU64>, timeout_ms: u64) {
    let wait_start = started.elapsed().as_millis() as u64;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = started.elapsed().as_millis() as u64;
        let last = last_output_ms.load(Ordering::Relaxed);
        if last > 0 && now.saturating_sub(last) >= PROMPT_IDLE_MS {
            return;
        }
        if now.saturating_sub(wait_start) > timeout_ms {
            return;
        }
    }
}

pub fn run(
    tasks: Vec<StartupTask>,
    vars: HashMap<String, String>,
    tx: mpsc::UnboundedSender<SessionCmd>,
    last_output_ms: Arc<AtomicU64>,
) {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        for task in tasks {
            match task {
                StartupTask::WaitForPrompt => {
                    settle(&started, &last_output_ms, PROMPT_TIMEOUT_MS).await;
                }
                StartupTask::Command { value } => {
                    // Every command waits for the terminal to go quiet before it
                    // types, which is what a person does and what the explicit
                    // wait_for_prompt task only did when someone remembered to
                    // add one.
                    //
                    // The failure it fixes: `claude` followed by `/usage` put the
                    // second line into the tty while zsh still owned it and
                    // Claude was still starting. Claude ends up with the text but
                    // not the Enter — the slash command sits in the composer
                    // unsent. Measured on a real session: the input is swallowed
                    // for ~1.6s after `claude`, and the same bytes submit
                    // normally once it has drawn its prompt.
                    //
                    // Nothing is lost by waiting even when the far side is a
                    // plain shell: a prompt that has been silent for 600ms is a
                    // prompt that is ready.
                    settle(&started, &last_output_ms, SETTLE_TIMEOUT_MS).await;

                    let cmd = variables::substitute(&value, &vars);
                    let mut bytes = cmd.into_bytes();
                    // CR, not LF: this has to be the byte the Enter key sends,
                    // because that is all the far side ever sees. A unix tty
                    // accepts either (ICRNL turns the CR into a newline), but
                    // ConPTY replays input as key events, and LF arrives as
                    // Ctrl+J — which PSReadLine and cmd.exe both ignore. The
                    // line was landing in the prompt and sitting there.
                    bytes.push(b'\r');
                    if tx.send(SessionCmd::Data(bytes)).is_err() {
                        return; // session closed, stop the task list
                    }
                }
                StartupTask::Wait { milliseconds } => {
                    tokio::time::sleep(Duration::from_millis(milliseconds)).await;
                }
            }
        }
    });
}
