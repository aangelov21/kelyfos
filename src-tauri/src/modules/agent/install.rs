//! Installing and removing the Claude Code hook bridge.
//!
//! This is the one place Kelyfos writes to a file it does not own, so every
//! rule here exists to keep that safe on someone else's machine: back up before
//! the first write, preserve every unrelated key, never touch hooks another
//! tool installed, and remove exactly what we added and nothing more.
//!
//! Kelyfos entries are recognised by the shim path inside their command rather
//! than by a marker field. Claude validates its own config, and an unknown key
//! we invented is a bet on how it treats extra fields; a path we control is not.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

/// Bumped when the shim script changes, so an updated app rewrites it.
const SHIM_VERSION: u32 = 1;

/// Hook events Kelyfos listens to. PreToolUse and PostToolUse take a tool
/// matcher; the rest fire once and take none.
const EVENTS_WITH_MATCHER: [&str; 2] = ["PreToolUse", "PostToolUse"];
const EVENTS_PLAIN: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "Stop",
    "SubagentStop",
    "SessionEnd",
];

pub fn claude_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude"))
}

pub fn settings_path() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("settings.json"))
}

/// The shim lives in the Kelyfos data directory, never inside the app bundle:
/// settings.json stores this path as text, and a path into the bundle would
/// break for every user the first time the app updates or is moved.
///
/// Two names, because the two platforms cannot run each other's script. They
/// are deliberately distinct rather than one `claude-hook` with different
/// contents: the extension is what tells the reader — and PowerShell — which
/// one they are looking at.
#[cfg(windows)]
pub fn shim_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("kelyfos").join("claude-hook.ps1"))
}

#[cfg(not(windows))]
pub fn shim_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("kelyfos").join("claude-hook.sh"))
}

pub fn shim_installed() -> bool {
    shim_path().is_some_and(|p| p.is_file())
}

/// Write the shim if missing or stale. Idempotent.
pub fn ensure_shim() -> Result<PathBuf, String> {
    let path = shim_path().ok_or("No data directory available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let wanted = shim_source();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current != wanted {
        std::fs::write(&path, &wanted).map_err(|e| format!("Could not write the hook script: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // Builds before the named pipe wrote a .sh here that Windows could never
    // run. It is inert, but a stray hook script in the data folder invites the
    // question of which one is live, so the dead one goes.
    #[cfg(windows)]
    if let Some(dir) = path.parent() {
        std::fs::remove_file(dir.join("claude-hook.sh")).ok();
    }
    Ok(path)
}

#[cfg(not(windows))]
fn shim_source() -> String {
    format!(
        r#"#!/bin/sh
# Kelyfos ↔ Claude Code bridge (v{SHIM_VERSION}) — generated file, edits are overwritten.
#
# Forwards one hook event to the running Kelyfos app, then gets out of the way.
# It must never block, prompt or fail loudly: this runs inside the user's Claude
# session, and no status indicator is worth degrading that. Every path below
# ends in exit 0.

sock="${{KELYFOS_HOOK_SOCKET:-$HOME/Library/Application Support/kelyfos/hooks.sock}}"

# Kelyfos closed, or Claude started outside a Kelyfos terminal: nothing to tell.
[ -S "$sock" ] || exit 0
[ -n "$KELYFOS_TERMINAL_ID" ] || exit 0

payload=$(cat)
[ -n "$payload" ] || exit 0

message="{{\"terminalId\":\"$KELYFOS_TERMINAL_ID\",\"event\":$payload}}"

if command -v nc >/dev/null 2>&1; then
  printf '%s\n' "$message" | nc -U "$sock" >/dev/null 2>&1 &
elif command -v python3 >/dev/null 2>&1; then
  printf '%s\n' "$message" | python3 -c '
import socket, sys, os
try:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(1)
    s.connect(os.environ["KELYFOS_SOCK"])
    s.sendall(sys.stdin.buffer.read())
    s.close()
except Exception:
    pass
' >/dev/null 2>&1 &
fi

exit 0
"#
    )
}

/// The Windows shim. Same contract as the sh one — read the payload on stdin,
/// wrap it with the terminal id, hand it to the app, never fail loudly — over a
/// named pipe instead of a socket.
///
/// PowerShell rather than a .cmd because a batch file has no way to open a pipe
/// or to read a multi-line payload from stdin without contortions, and .NET's
/// NamedPipeClientStream is present on every Windows install back to 7. The
/// version line is the only substitution, so the body stays a plain string: the
/// script is mostly braces, and `format!` would mean doubling every one of them.
#[cfg(windows)]
fn shim_source() -> String {
    format!(
        "# Kelyfos <-> Claude Code bridge (v{SHIM_VERSION}) - generated file, edits are overwritten.\n{SHIM_PS1}"
    )
}

/// Nothing here may throw, prompt or write to stdout: this runs inside the
/// user's Claude session on every tool call, and no status indicator is worth
/// degrading that. The catch-all and the final `exit 0` are the guarantee.
#[cfg(windows)]
const SHIM_PS1: &str = r#"#
# Forwards one hook event to the running Kelyfos app, then gets out of the way.

$ErrorActionPreference = 'SilentlyContinue'
try {
    $endpoint = $env:KELYFOS_HOOK_SOCKET
    $terminal = $env:KELYFOS_TERMINAL_ID
    # Kelyfos closed, or Claude started outside a Kelyfos terminal.
    if (-not $endpoint -or -not $terminal) { exit 0 }

    $payload = [Console]::In.ReadToEnd()
    if (-not $payload) { exit 0 }

    # The client API wants the bare name, not the \\.\pipe\ prefix the app
    # hands out. Everything else is passed through untouched.
    $name = $endpoint -replace '^\\\\\.\\pipe\\', ''

    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream '.', $name, ([System.IO.Pipes.PipeDirection]::Out)
    # A local pipe with an instance waiting connects in well under a
    # millisecond, so this budget is only ever spent when nothing is listening
    # at all - a second Kelyfos that lost the race for the name, or an app that
    # was killed while its terminals kept running. Connect polls for the whole
    # timeout in that case rather than failing fast, and it is measurably the
    # difference between a 370ms and a 1.2s hook. Give up quickly instead.
    $pipe.Connect(200)
    $message = '{"terminalId":"' + $terminal + '","event":' + $payload + '}'
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($message)
    $pipe.Write($bytes, 0, $bytes.Length)
    $pipe.Flush()
    $pipe.Dispose()
} catch { }

exit 0
"#;

/// True when this hook entry is one of ours.
///
/// Both script names, because removal has to catch every entry any build ever
/// wrote. A Windows machine can be carrying .sh entries from before the named
/// pipe existed, and a synced ~/.claude can carry the other platform's.
fn is_ours(entry: &Value) -> bool {
    entry.get("command").and_then(Value::as_str).is_some_and(|c| {
        c.contains("kelyfos") && (c.contains("claude-hook.sh") || c.contains("claude-hook.ps1"))
    })
}

fn read_settings(path: &PathBuf) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {path:?}: {e}"))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    // Refusing to touch a file we cannot parse is the only safe move: rewriting
    // it would discard settings the user still has, and they would have no way
    // to know Kelyfos was responsible.
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("~/.claude/settings.json is not a JSON object.".into()),
        Err(e) => Err(format!(
            "~/.claude/settings.json is not valid JSON ({e}). Fix or remove it, then try again."
        )),
    }
}

/// Claude's own "theme" setting, read for display only — never written: which
/// theme Claude uses is its business, Kelyfos only wants to know whether it is
/// an `*-ansi` one (those draw with the 16 ANSI colors, so the terminal
/// palette recolors Claude; the others paint fixed colors the palette cannot
/// touch). None means the file is missing, unparsable, or has no theme key —
/// Claude then uses its default, which is not an ANSI theme.
pub fn theme() -> Option<String> {
    let path = settings_path()?;
    let map = read_settings(&path).ok()?;
    map.get("theme").and_then(Value::as_str).map(str::to_owned)
}

/// The custom theme Kelyfos offers Claude, written to ~/.claude/themes on a
/// button press — never on launch, and never selected on the user's behalf:
/// `settings.json`'s "theme" key stays untouched, switching is done with
/// /theme inside Claude.
///
/// Why it exists: Claude's stock `dark-ansi` theme leaves several roles on
/// fixed colors and hands ANSI white to dim text *and* to the hover/expanded
/// block background — one palette slot, two jobs — so recoloring "white" in
/// the terminal palette bleeds across both. This theme pins every role it can
/// to a named ANSI slot, so the whole of Claude follows the terminal palette
/// and each role can be aimed at a slot the user actually wants for it;
/// background roles sit on `ansi:black`, which terminal themes design as a
/// near-background shade rather than as text.
///
/// Values are `ansi:<name>` on purpose. Claude also accepts rgb()/#hex, but a
/// fixed color here would be a color the palette cannot reach, which is the
/// whole thing this file exists to avoid.
const THEME_JSON: &str = r#"{
  "name": "kelyfos-theme",
  "base": "dark-ansi",
  "overrides": {
    "autoAccept": "ansi:magentaBright",
    "autoAcceptShimmer": "ansi:magentaBright",
    "background": "ansi:cyanBright",
    "bashBorder": "ansi:magentaBright",
    "bashMessageBackgroundColor": "ansi:black",
    "blue_FOR_SUBAGENTS_ONLY": "ansi:blueBright",
    "briefLabelClaude": "ansi:redBright",
    "briefLabelYou": "ansi:blueBright",
    "chromeYellow": "ansi:yellowBright",
    "claude": "ansi:magenta",
    "claudeBlue_FOR_SYSTEM_SPINNER": "ansi:blueBright",
    "claudeBlueShimmer_FOR_SYSTEM_SPINNER": "ansi:blueBright",
    "claudeShimmer": "ansi:yellowBright",
    "clawd_background": "ansi:black",
    "clawd_body": "ansi:red",
    "composerSidebarBackground": "ansi:black",
    "cyan_FOR_SUBAGENTS_ONLY": "ansi:cyanBright",
    "diffAdded": "ansi:green",
    "diffAddedDimmed": "ansi:green",
    "diffAddedWord": "ansi:greenBright",
    "diffRemoved": "ansi:red",
    "diffRemovedDimmed": "ansi:red",
    "diffRemovedWord": "ansi:redBright",
    "effortUltra": "ansi:magentaBright",
    "error": "ansi:redBright",
    "fastMode": "ansi:redBright",
    "fastModeShimmer": "ansi:redBright",
    "green_FOR_SUBAGENTS_ONLY": "ansi:greenBright",
    "ide": "ansi:blue",
    "inactive": "ansi:blackBright",
    "inactiveShimmer": "ansi:whiteBright",
    "inverseText": "rgb(10, 11, 12)",
    "memoryBackgroundColor": "ansi:black",
    "merged": "ansi:magentaBright",
    "orange_FOR_SUBAGENTS_ONLY": "ansi:redBright",
    "permission": "ansi:magenta",
    "permissionShimmer": "ansi:blueBright",
    "pink_FOR_SUBAGENTS_ONLY": "ansi:magentaBright",
    "planMode": "ansi:cyanBright",
    "professionalBlue": "ansi:blueBright",
    "promptBorder": "ansi:white",
    "promptBorderShimmer": "ansi:whiteBright",
    "purple_FOR_SUBAGENTS_ONLY": "ansi:magentaBright",
    "rainbow_blue": "ansi:cyan",
    "rainbow_blue_shimmer": "ansi:cyanBright",
    "rainbow_green": "ansi:green",
    "rainbow_green_shimmer": "ansi:greenBright",
    "rainbow_indigo": "ansi:blue",
    "rainbow_indigo_shimmer": "ansi:blueBright",
    "rainbow_orange": "ansi:redBright",
    "rainbow_orange_shimmer": "ansi:yellow",
    "rainbow_red": "ansi:red",
    "rainbow_red_shimmer": "ansi:redBright",
    "rainbow_violet": "ansi:magenta",
    "rainbow_violet_shimmer": "ansi:magentaBright",
    "rainbow_yellow": "ansi:yellow",
    "rainbow_yellow_shimmer": "ansi:yellowBright",
    "rate_limit_empty": "ansi:black",
    "rate_limit_fill": "ansi:magenta",
    "red_FOR_SUBAGENTS_ONLY": "ansi:redBright",
    "remember": "ansi:blueBright",
    "selectionBg": "ansi:black",
    "skill": "ansi:magentaBright",
    "subtle": "ansi:white",
    "success": "ansi:greenBright",
    "suggestion": "ansi:magenta",
    "text": "ansi:whiteBright",
    "userMessageBackground": "ansi:black",
    "userMessageBackgroundHover": "ansi:black",
    "warning": "ansi:yellowBright",
    "warningShimmer": "ansi:yellowBright",
    "yellow_FOR_SUBAGENTS_ONLY": "ansi:yellowBright"
  }
}
"#;

/// Claude selects it as "custom:kelyfos-theme" — derived from the file stem.
pub fn theme_path() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("themes").join("kelyfos-theme.json"))
}

/// Installed *and current*: a stale version or a hand edit reads as not
/// installed, so the UI offers the install button again instead of silently
/// leaving an old mapping behind.
pub fn theme_installed() -> bool {
    theme_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|text| text == THEME_JSON)
}

pub fn install_theme() -> Result<(), String> {
    let path = theme_path().ok_or("No home directory available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, THEME_JSON).map_err(|e| format!("Could not write {path:?}: {e}"))
}

pub fn remove_theme() -> Result<(), String> {
    let path = theme_path().ok_or("No home directory available")?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not remove {path:?}: {e}")),
    }
}

fn write_settings(path: &PathBuf, map: &Map<String, Value>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(map.clone()))
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| format!("Could not write {path:?}: {e}"))
}

/// Copy the file aside before the first modification, so a user always has the
/// original to fall back to.
fn backup_settings(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = path.with_file_name(format!("settings.json.kelyfos-{stamp}.bak"));
    std::fs::copy(path, &backup).map_err(|e| format!("Could not back up settings.json: {e}"))?;
    Ok(())
}

/// Strip every Kelyfos entry, leaving other tools' hooks exactly as they were
/// and dropping the containers that our removal emptied.
fn strip_ours(hooks: &mut Map<String, Value>) {
    let event_names: Vec<String> = hooks.keys().cloned().collect();
    for event in event_names {
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|entry| !is_ours(entry));
            }
        }
        // A group whose hook list we emptied was ours alone; one that still has
        // entries belongs to somebody else and stays untouched.
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|l| !l.is_empty())
        });
        if groups.is_empty() {
            hooks.remove(&event);
        }
    }
}

/// Quote a path for the shell Claude runs hook commands in. The default data
/// directory on macOS is "~/Library/Application Support/kelyfos", so an
/// unquoted path splits on the space and the hook dies before it starts.
#[cfg(not(windows))]
fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', r"'\''"))
}

#[cfg(not(windows))]
fn hook_command(shim: &str) -> String {
    shell_quote(shim)
}

/// Windows runs hook commands through cmd.exe, which cannot execute a .ps1 by
/// naming it — hence the explicit interpreter. `powershell` and not `pwsh`:
/// Windows PowerShell is on every machine, PowerShell 7 is not.
///
/// The flags are not decoration. -NoProfile keeps the user's profile out of a
/// path that runs on every tool call, and profiles are routinely slow enough to
/// be felt. -NonInteractive means the script can never stop for a prompt inside
/// a Claude session. -ExecutionPolicy Bypass is required because the default
/// policy on a workstation is Restricted, which refuses unsigned scripts — and
/// it applies to this invocation only, not to the machine.
///
/// Double quotes because cmd.exe does not understand single ones; a Windows
/// path cannot contain a double quote, so there is nothing left to escape.
#[cfg(windows)]
fn hook_command(shim: &str) -> String {
    format!("powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{shim}\"")
}

fn hook_entry(shim: &str) -> Value {
    json!({ "type": "command", "command": hook_command(shim) })
}

/// Add the Kelyfos hooks to ~/.claude/settings.json. Idempotent: an install
/// over an existing one replaces our entries and leaves everything else alone.
pub fn install() -> Result<(), String> {
    let shim = ensure_shim()?;
    let path = settings_path().ok_or("No home directory available")?;
    install_at(&path, &shim.to_string_lossy())
}

/// The file work, with its paths passed in so it can be exercised against the
/// awkward configurations real users have without touching their home
/// directory.
fn install_at(path: &PathBuf, shim: &str) -> Result<(), String> {
    let mut settings = read_settings(path)?;
    backup_settings(path)?;

    let mut hooks = settings
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Clear any previous version of ours first, so upgrading never doubles up.
    strip_ours(&mut hooks);

    for event in EVENTS_WITH_MATCHER {
        let groups = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        if let Some(arr) = groups.as_array_mut() {
            arr.push(json!({ "matcher": "*", "hooks": [hook_entry(shim)] }));
        }
    }
    for event in EVENTS_PLAIN {
        let groups = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(vec![]));
        if let Some(arr) = groups.as_array_mut() {
            arr.push(json!({ "hooks": [hook_entry(shim)] }));
        }
    }

    settings.insert("hooks".into(), Value::Object(hooks));
    write_settings(path, &settings)
}

/// Remove the Kelyfos hooks. Leaves the file as it was before install where our
/// entries were the only change, and removes the shim script too.
pub fn uninstall() -> Result<(), String> {
    let path = settings_path().ok_or("No home directory available")?;
    uninstall_at(&path)?;
    if let Some(shim) = shim_path() {
        std::fs::remove_file(shim).ok();
    }
    Ok(())
}

fn uninstall_at(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut settings = read_settings(path)?;
    backup_settings(path)?;
    if let Some(mut hooks) = settings.get("hooks").and_then(Value::as_object).cloned() {
        strip_ours(&mut hooks);
        if hooks.is_empty() {
            settings.remove("hooks");
        } else {
            settings.insert("hooks".into(), Value::Object(hooks));
        }
    }
    write_settings(path, &settings)
}

/// How many of the expected events carry a Kelyfos hook in the form this build
/// writes. Deliberately stricter than `is_ours`: removal has to catch every
/// entry we ever wrote, but "installed" must mean the entry actually works, so
/// one left over from a build with a different command string reads as not
/// installed and the user is offered a repair.
pub fn installed_events() -> usize {
    let Some(path) = settings_path() else {
        return 0;
    };
    let Ok(settings) = read_settings(&path) else {
        return 0;
    };
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return 0;
    };
    let Some(expected) = shim_path().map(|p| hook_command(&p.to_string_lossy())) else {
        return 0;
    };
    hooks
        .values()
        .filter(|groups| {
            groups.as_array().is_some_and(|gs| {
                gs.iter().any(|g| {
                    g.get("hooks").and_then(Value::as_array).is_some_and(|l| {
                        l.iter().any(|e| {
                            e.get("command").and_then(Value::as_str) == Some(expected.as_str())
                        })
                    })
                })
            })
        })
        .count()
}

pub const EXPECTED_EVENT_COUNT: usize = EVENTS_WITH_MATCHER.len() + EVENTS_PLAIN.len();

#[cfg(test)]
mod tests {
    use super::*;

    const SHIM: &str = "/Users/x/Library/Application Support/kelyfos/claude-hook.sh";

    fn ours() -> Value {
        hook_entry(SHIM)
    }

    fn theirs() -> Value {
        json!({ "type": "command", "command": "/usr/local/bin/other-tool" })
    }

    /// THEME_JSON is a raw string, so a typo would only surface as Claude
    /// silently refusing the theme. Pin the parts its loader depends on: valid
    /// JSON, the name matching the file stem (that pairing is what /theme
    /// lists), and a base to inherit from.
    #[test]
    fn kelyfos_theme_is_a_valid_theme_file() {
        let v: Value = serde_json::from_str(THEME_JSON).expect("theme file must be valid JSON");
        assert_eq!(v["name"], "kelyfos-theme");
        assert_eq!(v["base"], "dark-ansi");
        assert!(v["overrides"].is_object());
    }

    /// The default data directory on macOS is "~/Library/Application
    /// Support/kelyfos". Written unquoted, /bin/sh splits it at the space and
    /// every hook fails with "No such file or directory".
    #[cfg(not(windows))]
    #[test]
    fn quotes_a_path_containing_spaces() {
        let entry = hook_entry("/Users/x/Library/Application Support/kelyfos/claude-hook.sh");
        assert_eq!(
            entry["command"],
            "'/Users/x/Library/Application Support/kelyfos/claude-hook.sh'"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn quotes_a_path_containing_a_quote() {
        assert_eq!(shell_quote("/Users/o'brien/x.sh"), r"'/Users/o'\''brien/x.sh'");
    }

    /// The Windows equivalent, and the same hazard: the default data directory
    /// is under AppData, but a machine whose user name has a space in it — very
    /// common — puts one in the path, and cmd.exe splits an unquoted one.
    #[cfg(windows)]
    #[test]
    fn runs_the_script_through_powershell_with_the_path_quoted() {
        let entry = hook_entry(r"C:\Users\Jo Smith\AppData\Roaming\kelyfos\claude-hook.ps1");
        assert_eq!(
            entry["command"],
            "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass \
             -File \"C:\\Users\\Jo Smith\\AppData\\Roaming\\kelyfos\\claude-hook.ps1\""
        );
    }

    /// A .ps1 entry has to be recognised as ours by the same rule that catches
    /// the .sh one, or an upgrade would stack a second set instead of replacing.
    #[test]
    fn recognises_both_platforms_scripts() {
        let ps = json!({
            "type": "command",
            "command": "powershell -NoProfile -File \"C:\\x\\kelyfos\\claude-hook.ps1\""
        });
        assert!(is_ours(&ps));
        assert!(is_ours(&ours()));
        // Someone else's PowerShell hook is still not ours.
        assert!(!is_ours(&json!({
            "type": "command",
            "command": "powershell -File \"C:\\x\\other-tool.ps1\""
        })));
    }

    /// Removal has to catch entries written by any build, quoted or not.
    #[test]
    fn recognises_an_unquoted_entry_from_an_older_build() {
        let legacy = json!({
            "type": "command",
            "command": "/Users/x/Library/Application Support/kelyfos/claude-hook.sh"
        });
        assert!(is_ours(&legacy));

        let mut hooks: Map<String, Value> =
            serde_json::from_value(json!({ "Stop": [{ "hooks": [legacy] }] })).unwrap();
        strip_ours(&mut hooks);
        assert!(hooks.is_empty());
    }

    #[test]
    fn recognises_only_our_entries() {
        assert!(is_ours(&ours()));
        assert!(!is_ours(&theirs()));
        assert!(!is_ours(&json!({ "type": "command" })));
    }

    /// The whole point of the marker: another tool's hooks survive uninstall.
    #[test]
    fn strip_keeps_other_tools_hooks() {
        let mut hooks: Map<String, Value> = serde_json::from_value(json!({
            "Stop": [{ "hooks": [ours(), theirs()] }],
            "PreToolUse": [{ "matcher": "*", "hooks": [ours()] }]
        }))
        .unwrap();

        strip_ours(&mut hooks);

        // Ours gone, theirs intact, and the event we emptied removed entirely.
        assert_eq!(hooks["Stop"][0]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(hooks["Stop"][0]["hooks"][0], theirs());
        assert!(!hooks.contains_key("PreToolUse"));
    }

    #[test]
    fn strip_is_a_no_op_without_our_entries() {
        let original = json!({ "Stop": [{ "hooks": [theirs()] }] });
        let mut hooks: Map<String, Value> = serde_json::from_value(original.clone()).unwrap();
        strip_ours(&mut hooks);
        assert_eq!(Value::Object(hooks), original);
    }

    #[test]
    fn reads_a_missing_file_as_empty() {
        let path = std::env::temp_dir().join("kelyfos-does-not-exist.json");
        assert_eq!(read_settings(&path).unwrap(), Map::new());
    }

    /// Malformed JSON must abort rather than be overwritten — the user's real
    /// settings are in there somewhere.
    #[test]
    fn refuses_malformed_json() {
        let path = std::env::temp_dir().join("kelyfos-malformed.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = read_settings(&path).unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn refuses_a_non_object_file() {
        let path = std::env::temp_dir().join("kelyfos-array.json");
        std::fs::write(&path, "[1, 2]").unwrap();
        assert!(read_settings(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    /// A throwaway directory, so the installer can be run against real files
    /// without going near the user's own ~/.claude.
    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "kelyfos-install-{tag}-{}",
                std::process::id()
            ));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn settings(&self) -> PathBuf {
            self.0.join("settings.json")
        }

        fn write(&self, text: &str) {
            std::fs::write(self.settings(), text).unwrap();
        }

        fn read(&self) -> Value {
            serde_json::from_str(&std::fs::read_to_string(self.settings()).unwrap()).unwrap()
        }

        fn backups(&self) -> usize {
            std::fs::read_dir(&self.0)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().contains(".kelyfos-"))
                .count()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn our_events(settings: &Value) -> Vec<String> {
        let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
            return vec![];
        };
        let mut names: Vec<String> = hooks
            .iter()
            .filter(|(_, groups)| {
                groups.as_array().is_some_and(|gs| {
                    gs.iter().any(|g| {
                        g["hooks"]
                            .as_array()
                            .is_some_and(|l| l.iter().any(is_ours))
                    })
                })
            })
            .map(|(k, _)| k.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn installs_into_a_missing_file() {
        let home = TempHome::new("missing");
        install_at(&home.settings(), SHIM).unwrap();
        assert_eq!(our_events(&home.read()).len(), EXPECTED_EVENT_COUNT);
        assert_eq!(home.backups(), 0, "nothing existed to back up");
    }

    /// The settings real users have: model, permissions, a status line. Losing
    /// any of it to a status-dot feature would be indefensible.
    #[test]
    fn preserves_unrelated_settings() {
        let home = TempHome::new("unrelated");
        home.write(
            r#"{
              "model": "opus",
              "permissions": { "defaultMode": "auto" },
              "statusLine": { "type": "command", "command": "sh x.sh" }
            }"#,
        );

        install_at(&home.settings(), SHIM).unwrap();

        let after = home.read();
        assert_eq!(after["model"], "opus");
        assert_eq!(after["permissions"]["defaultMode"], "auto");
        assert_eq!(after["statusLine"]["command"], "sh x.sh");
        assert_eq!(home.backups(), 1, "the original was copied aside first");
    }

    #[test]
    fn leaves_another_tools_hooks_alone() {
        let home = TempHome::new("othertool");
        home.write(r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/other-tool" }] }] } }"#);

        install_at(&home.settings(), SHIM).unwrap();

        let stop = home.read()["hooks"]["Stop"].clone();
        let commands: Vec<String> = stop
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap().clone())
            .map(|h| h["command"].as_str().unwrap().to_string())
            .collect();
        assert!(commands.iter().any(|c| c == "/usr/local/bin/other-tool"));
        assert!(commands.iter().any(|c| *c == hook_command(SHIM)));
    }

    /// Reconnecting, or an upgrade rewriting the hooks, must not stack a second
    /// copy of every entry.
    #[test]
    fn installing_twice_changes_nothing() {
        let home = TempHome::new("twice");
        install_at(&home.settings(), SHIM).unwrap();
        let once = home.read();

        install_at(&home.settings(), SHIM).unwrap();

        assert_eq!(home.read(), once);
    }

    #[test]
    fn uninstall_restores_the_original_exactly() {
        let home = TempHome::new("roundtrip");
        let original = r#"{
          "model": "opus",
          "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/other-tool" }] }] }
        }"#;
        home.write(original);
        let before: Value = serde_json::from_str(original).unwrap();

        install_at(&home.settings(), SHIM).unwrap();
        uninstall_at(&home.settings()).unwrap();

        assert_eq!(home.read(), before);
    }

    /// With nothing else in the file, uninstall leaves no empty scaffolding.
    #[test]
    fn uninstall_removes_the_hooks_key_it_created() {
        let home = TempHome::new("cleanup");
        home.write(r#"{ "model": "opus" }"#);

        install_at(&home.settings(), SHIM).unwrap();
        uninstall_at(&home.settings()).unwrap();

        let after = home.read();
        assert!(after.get("hooks").is_none());
        assert_eq!(after["model"], "opus");
    }

    /// The user's real settings are in there somewhere; overwriting them to
    /// install a status indicator is not a trade worth making.
    #[test]
    fn refuses_to_write_over_malformed_json() {
        let home = TempHome::new("malformed");
        home.write("{ not json at all");

        let err = install_at(&home.settings(), SHIM).unwrap_err();

        assert!(err.contains("not valid JSON"), "{err}");
        assert_eq!(
            std::fs::read_to_string(home.settings()).unwrap(),
            "{ not json at all",
            "the file must be untouched"
        );
    }

    #[test]
    fn uninstall_is_a_no_op_when_nothing_was_installed() {
        let home = TempHome::new("noop");
        home.write(r#"{ "model": "opus" }"#);
        uninstall_at(&home.settings()).unwrap();
        assert_eq!(home.read()["model"], "opus");
    }

    #[cfg(not(windows))]
    #[test]
    fn shim_never_exits_nonzero() {
        let src = shim_source();
        assert!(src.contains("exit 0"));
        assert!(src.contains("[ -S \"$sock\" ] || exit 0"));
        assert!(src.contains("KELYFOS_TERMINAL_ID"));
    }

    /// Same contract on Windows, enforced differently: PowerShell has no `set
    /// -e`, so what stands in for it is a catch-all around the whole body and an
    /// explicit exit. A hook that surfaced an error would put a red line in the
    /// user's Claude session over a status dot.
    #[cfg(windows)]
    #[test]
    fn shim_never_exits_nonzero() {
        let src = shim_source();
        assert!(src.contains("exit 0"));
        assert!(src.contains("} catch { }"));
        assert!(src.contains("$ErrorActionPreference = 'SilentlyContinue'"));
        assert!(src.contains("if (-not $endpoint -or -not $terminal) { exit 0 }"));
        assert!(src.contains("KELYFOS_TERMINAL_ID"));
    }

    /// The wrapper the listener parses. Written by hand in each shim, so a typo
    /// here is a bridge that connects and then silently drops everything.
    #[cfg(windows)]
    #[test]
    fn shim_wraps_the_payload_with_the_terminal_id() {
        assert!(shim_source()
            .contains(r#"'{"terminalId":"' + $terminal + '","event":' + $payload + '}'"#));
    }

    /// The whole Windows chain, for real: the generated script, run the way
    /// Claude runs it, writing to a pipe of the kind the app opens.
    ///
    /// Everything above this checks a string against another string. None of it
    /// would catch the failures that actually bite here — a PowerShell parse
    /// error, an execution policy refusing the file, a pipe name the client
    /// rejects, or a payload that arrives malformed. Those only show up when the
    /// script is really executed, which is why this test pays for a process.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_generated_shim_delivers_over_a_real_pipe() {
        use std::io::Write;
        use tokio::io::AsyncReadExt;
        use tokio::net::windows::named_pipe::ServerOptions;

        // Named after this process, so a Kelyfos running on the same machine —
        // very likely, during development — is never the thing that answers.
        let name = format!(r"\\.\pipe\kelyfos-hooks-test-{}", std::process::id());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .expect("could not open the test pipe");

        let shim = std::env::temp_dir().join(format!("kelyfos-shim-{}.ps1", std::process::id()));
        std::fs::write(&shim, shim_source()).unwrap();

        let mut child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&shim)
            .env("KELYFOS_HOOK_SOCKET", &name)
            .env("KELYFOS_TERMINAL_ID", "t-abc")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("powershell should be on every Windows machine");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"hook_event_name":"Notification","session_id":"s1"}"#)
            .unwrap();

        let mut buf = Vec::new();
        // Generous: this waits for a whole PowerShell startup, which is slow
        // enough on a cold machine to make a tight bound flaky.
        let received = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            server.connect().await?;
            server.take(64 * 1024).read_to_end(&mut buf).await
        })
        .await;

        let status = child.wait();
        std::fs::remove_file(&shim).ok();

        received
            .expect("the shim never reached the pipe")
            .expect("the pipe carried nothing");
        assert!(status.unwrap().success(), "the shim must always exit 0");

        let msg: Value = serde_json::from_slice(&buf).expect("the shim sent malformed JSON");
        assert_eq!(msg["terminalId"], "t-abc");
        assert_eq!(msg["event"]["hook_event_name"], "Notification");
        assert_eq!(msg["event"]["session_id"], "s1");
    }
}
