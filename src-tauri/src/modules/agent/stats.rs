//! Reading Claude Code session history off disk.
//!
//! Claude keeps one JSONL transcript per session under
//! ~/.claude/projects/<slug>/<session-id>.jsonl, where the slug is the working
//! directory with its separators flattened. A live session's file grows to
//! megabytes, and the status bar re-reads it every few seconds, so parsing is
//! incremental: each file's byte offset and running totals are kept, and only
//! the bytes appended since last time are parsed.
//!
//! These are Claude Code's own files, not a public API — the shapes here can
//! change between its versions, which is why every field is optional and an
//! unrecognised line is skipped rather than treated as an error.
//!
//! Transcripts contain the user's source code verbatim (old and new strings of
//! every edit, command output, whatever was pasted). Nothing here leaves the
//! machine, and none of it may enter a backup or an export.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Claude's own directory name for a working directory: every character that is
/// not alphanumeric becomes a dash, so /Users/x/Sites/my app becomes
/// -Users-x-Sites-my-app.
pub fn project_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

// ---------- Accumulator ----------

/// Running totals for one transcript. Everything the status bar can show is
/// derived from here, so a new statistic means one more field and one more
/// branch in `absorb` rather than another pass over the file.
#[derive(Default, Clone)]
struct Totals {
    user_messages: u64,
    prompts: u64,
    assistant_messages: u64,
    tool_calls: u64,
    tool_errors: u64,
    tool_names: HashMap<String, u64>,

    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,

    lines_added: u64,
    lines_removed: u64,
    files: HashMap<String, u64>,
    user_modified_after_edit: u64,
    interrupted_commands: u64,

    compactions: u64,
    turns: u64,
    total_turn_ms: u64,
    longest_turn_ms: u64,
    sidechain_messages: u64,
    end_turns: u64,
    tool_use_stops: u64,

    models: Vec<String>,
    claude_version: Option<String>,
    transcript_git_branch: Option<String>,
    cwd: Option<String>,
    custom_title: Option<String>,
    first_prompt: Option<String>,
    started_at: Option<i64>,
    updated_at: Option<i64>,
}

/// One transcript line. Every field optional: Claude adds and removes them
/// between versions, and a line we only half understand is still worth counting.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    #[serde(default)]
    #[serde(rename = "isMeta")]
    is_meta: bool,
    #[serde(default)]
    #[serde(rename = "isSidechain")]
    is_sidechain: bool,
    timestamp: Option<String>,
    cwd: Option<String>,
    version: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
    message: Option<Message>,
    /// Deliberately untyped. A tool result is usually an object, but a rejected
    /// or blocked call arrives as a bare string and some tools return an array.
    /// Typed as a struct, those shapes fail to deserialize and serde discards
    /// the *whole line* — which silently lost 27 user messages here, and the
    /// lost ones were exactly the failures worth counting.
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Message {
    model: Option<String>,
    #[serde(rename = "stop_reason")]
    stop_reason: Option<String>,
    usage: Option<Usage>,
    content: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

fn flag(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn epoch_ms(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|t| t.timestamp_millis())
}

impl Totals {
    fn absorb(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Entry>(line) else {
            return;
        };

        if let Some(ms) = entry.timestamp.as_deref().and_then(epoch_ms) {
            self.started_at.get_or_insert(ms);
            self.updated_at = Some(ms);
        }
        if entry.cwd.is_some() {
            self.cwd = entry.cwd.clone();
        }
        if entry.version.is_some() {
            self.claude_version = entry.version.clone();
        }
        // Recorded as it appears, but see `git_branch` below: in practice this
        // is very often the literal "HEAD" rather than a branch name.
        if entry.git_branch.is_some() {
            self.transcript_git_branch = entry.git_branch.clone();
        }
        if entry.custom_title.is_some() {
            self.custom_title = entry.custom_title.clone();
        }
        if entry.is_sidechain {
            self.sidechain_messages += 1;
        }

        if entry.kind.as_deref() == Some("system") {
            match entry.subtype.as_deref() {
                Some("compact_boundary") => self.compactions += 1,
                Some("turn_duration") => {
                    self.turns += 1;
                    let ms = entry.duration_ms.unwrap_or(0);
                    self.total_turn_ms += ms;
                    self.longest_turn_ms = self.longest_turn_ms.max(ms);
                }
                _ => {}
            }
        }

        // Only the object form carries file and patch data; the string and
        // array forms are still perfectly valid lines and simply have none.
        if let Some(result) = entry.tool_use_result.as_ref() {
            if let Some(path) = result.get("filePath").and_then(serde_json::Value::as_str) {
                *self.files.entry(path.to_string()).or_insert(0) += 1;
            }
            if flag(result, "userModified") {
                self.user_modified_after_edit += 1;
            }
            if flag(result, "interrupted") {
                self.interrupted_commands += 1;
            }
            if let Some(patches) = result
                .get("structuredPatch")
                .and_then(serde_json::Value::as_array)
            {
                for lines in patches
                    .iter()
                    .filter_map(|p| p.get("lines").and_then(serde_json::Value::as_array))
                {
                    for l in lines.iter().filter_map(serde_json::Value::as_str) {
                        // A unified-diff hunk marks context with a space, so
                        // only the +/- rows are real changes.
                        if l.starts_with('+') {
                            self.lines_added += 1;
                        } else if l.starts_with('-') {
                            self.lines_removed += 1;
                        }
                    }
                }
            }
        }

        // Meta lines are the command wrappers and caveats Claude injects, not
        // anything the user or the model actually said.
        if entry.is_meta {
            return;
        }

        match entry.kind.as_deref() {
            Some("user") => {
                self.user_messages += 1;
                let content = entry.message.as_ref().and_then(|m| m.content.as_ref());
                // Most "user" lines are not the user talking: every tool result
                // comes back on one. In this repo's own session that is 1,116
                // of 1,209, so counting the type alone overstates prompts by
                // more than tenfold.
                if !carries_tool_result(content) {
                    self.prompts += 1;
                    if self.first_prompt.is_none() {
                        self.first_prompt =
                            content.and_then(first_text).map(|t| truncate(&t, 120));
                    }
                }
            }
            Some("assistant") => {
                self.assistant_messages += 1;
                if let Some(m) = entry.message.as_ref() {
                    // A session can span a model change; both are worth seeing.
                    if let Some(model) = m.model.as_ref() {
                        if model != "<synthetic>" && !self.models.contains(model) {
                            self.models.push(model.clone());
                        }
                    }
                    match m.stop_reason.as_deref() {
                        Some("end_turn") => self.end_turns += 1,
                        Some("tool_use") => self.tool_use_stops += 1,
                        _ => {}
                    }
                    if let Some(u) = m.usage.as_ref() {
                        self.input_tokens += u.input_tokens;
                        self.output_tokens += u.output_tokens;
                        self.cache_read_tokens += u.cache_read_input_tokens;
                        self.cache_creation_tokens += u.cache_creation_input_tokens;
                    }
                    self.count_blocks(m.content.as_ref());
                }
            }
            _ => {}
        }
    }

    fn count_blocks(&mut self, content: Option<&serde_json::Value>) {
        let Some(serde_json::Value::Array(items)) = content else {
            return;
        };
        for item in items {
            match item.get("type").and_then(serde_json::Value::as_str) {
                Some("tool_use") => {
                    self.tool_calls += 1;
                    if let Some(name) = item.get("name").and_then(serde_json::Value::as_str) {
                        *self.tool_names.entry(name.to_string()).or_insert(0) += 1;
                    }
                }
                Some("tool_result") if flag(item, "is_error") => self.tool_errors += 1,
                _ => {}
            }
        }
    }
}

/// Whether this content is a tool result being handed back to the model rather
/// than something the user typed.
fn carries_tool_result(content: Option<&serde_json::Value>) -> bool {
    let Some(serde_json::Value::Array(items)) = content else {
        return false;
    };
    items
        .iter()
        .any(|i| i.get("type").and_then(serde_json::Value::as_str) == Some("tool_result"))
}

/// Message content is either a plain string or a list of typed blocks.
fn first_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(items) => items.iter().find_map(|item| {
            (item.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .then(|| item.get("text").and_then(serde_json::Value::as_str))
                .flatten()
                .map(str::to_string)
        }),
        _ => None,
    }
}

fn truncate(text: &str, max: usize) -> String {
    let text = text.trim().replace('\n', " ");
    if text.chars().count() <= max {
        return text;
    }
    text.chars().take(max).collect::<String>() + "…"
}

// ---------- Incremental reading ----------

struct Cached {
    /// Bytes already folded into `totals`. Only whole lines are ever counted.
    offset: u64,
    totals: Totals,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Fold everything appended since the last call into the running totals.
///
/// The tail of a live file is very often a half-written line, so parsing stops
/// at the last newline and the offset stays behind it — the remainder is read
/// again next time, when it is complete.
fn totals_for(path: &Path) -> Option<Totals> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();

    let mut guard = cache().lock().ok()?;
    let entry = guard.entry(path.to_path_buf()).or_insert_with(|| Cached {
        offset: 0,
        totals: Totals::default(),
    });

    // A file that shrank is a different file: Claude rotated or rewrote it, and
    // the totals we hold describe content that is no longer there.
    if len < entry.offset {
        entry.offset = 0;
        entry.totals = Totals::default();
    }
    if len == entry.offset {
        return Some(entry.totals.clone());
    }

    // Read in chunks but keep going until the file is consumed. Stopping after
    // one chunk would report partial totals as if they were final — a 27 MB
    // transcript would show roughly half its messages, with nothing to say so.
    let mut consumed_now = 0u64;
    while entry.offset < len && consumed_now < MAX_PER_CALL {
        file.seek(SeekFrom::Start(entry.offset)).ok()?;
        let mut buf = Vec::new();
        file.by_ref().take(CHUNK).read_to_end(&mut buf).ok()?;
        if buf.is_empty() {
            break;
        }

        let Some(last_newline) = buf.iter().rposition(|b| *b == b'\n') else {
            // Only a partial line so far. It is read again next time, when it
            // is complete — the offset deliberately stays behind it.
            break;
        };
        let complete = &buf[..=last_newline];
        for line in complete.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            if let Ok(text) = std::str::from_utf8(line) {
                entry.totals.absorb(text);
            }
        }
        entry.offset += complete.len() as u64;
        consumed_now += complete.len() as u64;
    }
    Some(entry.totals.clone())
}

/// Bytes per read. Only a memory ceiling — the loop above keeps going.
const CHUNK: u64 = 8 * 1024 * 1024;

/// Work ceiling for one call, so a pathological file cannot pin a thread. Far
/// above any real transcript; the largest here is about 27 MB.
const MAX_PER_CALL: u64 = 256 * 1024 * 1024;

// ---------- Public shapes ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEdits {
    pub path: String,
    pub edits: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolUse {
    pub name: String,
    pub count: u64,
}

/// Everything derivable from one transcript. Deliberately wide — trimming the
/// display is a UI decision, and re-reading the file to add a number back is
/// the expensive way to find out you wanted it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub id: String,
    pub path: String,
    pub size_bytes: u64,

    // Timing
    /// First and last timestamp in the file, as epoch ms.
    pub started_at: Option<i64>,
    pub updated_at: Option<i64>,
    /// Wall clock from the first message to the last — includes the time the
    /// session sat idle, which is what "how long have I had this open" means.
    pub elapsed_ms: Option<i64>,
    /// Sum of Claude's own reported turn durations: time actually spent
    /// working, which is usually a small fraction of the elapsed time.
    pub working_ms: u64,
    pub turns: u64,
    pub longest_turn_ms: u64,

    // Conversation
    /// Things the user actually said. Far fewer than `user_messages`, which
    /// counts every line of role "user" — tool results included.
    pub prompts: u64,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub top_tools: Vec<ToolUse>,
    pub end_turns: u64,
    pub tool_use_stops: u64,
    pub sidechain_messages: u64,
    /// Times the conversation was compacted — a proxy for context pressure.
    pub compactions: u64,

    // Consumption
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,

    // Code churn
    pub lines_added: u64,
    pub lines_removed: u64,
    pub files_touched: u64,
    pub top_files: Vec<FileEdits>,
    /// Files the user edited after Claude did — a rework signal.
    pub user_modified_after_edit: u64,
    pub interrupted_commands: u64,

    // Identity
    pub models: Vec<String>,
    pub claude_version: Option<String>,
    pub cwd: Option<String>,
    /// Branch as recorded in the transcript. Frequently the literal "HEAD"
    /// rather than a name, so `git_branch` is the one to show a user.
    pub transcript_git_branch: Option<String>,
    pub custom_title: Option<String>,
    pub first_prompt: Option<String>,
}

fn build(path: &Path, totals: Totals) -> Option<SessionStats> {
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let now = chrono::Utc::now().timestamp_millis();

    let mut top_files: Vec<FileEdits> = totals
        .files
        .iter()
        .map(|(path, edits)| FileEdits { path: path.clone(), edits: *edits })
        .collect();
    top_files.sort_by(|a, b| b.edits.cmp(&a.edits).then(a.path.cmp(&b.path)));
    top_files.truncate(10);

    let mut top_tools: Vec<ToolUse> = totals
        .tool_names
        .iter()
        .map(|(name, count)| ToolUse { name: name.clone(), count: *count })
        .collect();
    top_tools.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    top_tools.truncate(10);

    Some(SessionStats {
        id: path.file_stem()?.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        size_bytes,
        started_at: totals.started_at,
        updated_at: totals.updated_at,
        // Measured to now rather than to the last message: a session open in
        // front of the user is still running between turns.
        elapsed_ms: totals.started_at.map(|start| now - start),
        working_ms: totals.total_turn_ms,
        turns: totals.turns,
        longest_turn_ms: totals.longest_turn_ms,
        prompts: totals.prompts,
        user_messages: totals.user_messages,
        assistant_messages: totals.assistant_messages,
        tool_calls: totals.tool_calls,
        tool_errors: totals.tool_errors,
        top_tools,
        end_turns: totals.end_turns,
        tool_use_stops: totals.tool_use_stops,
        sidechain_messages: totals.sidechain_messages,
        compactions: totals.compactions,
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cache_read_tokens: totals.cache_read_tokens,
        cache_creation_tokens: totals.cache_creation_tokens,
        lines_added: totals.lines_added,
        lines_removed: totals.lines_removed,
        files_touched: totals.files.len() as u64,
        top_files,
        user_modified_after_edit: totals.user_modified_after_edit,
        interrupted_commands: totals.interrupted_commands,
        models: totals.models,
        claude_version: totals.claude_version,
        cwd: totals.cwd,
        transcript_git_branch: totals.transcript_git_branch,
        custom_title: totals.custom_title,
        first_prompt: totals.first_prompt,
    })
}

// ---------- Commands ----------

/// Stats for one transcript, by path. The live session's path arrives with the
/// hook events, so the status bar never has to guess which file is current.
#[tauri::command]
pub async fn claude_session_stats(path: String) -> Option<SessionStats> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return None;
    }
    let totals = totals_for(&path)?;
    build(&path, totals)
}

/// Sessions Claude has recorded for a working directory, newest first.
#[tauri::command]
pub async fn claude_sessions(cwd: String, limit: Option<usize>) -> Vec<SessionStats> {
    let limit = limit.unwrap_or(10);
    let Some(dir) = projects_dir().map(|d| d.join(project_slug(&cwd))) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };

    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let modified = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;
            Some((e.path(), modified))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    files.truncate(limit);

    files
        .into_iter()
        .filter_map(|(path, _)| {
            let totals = totals_for(&path)?;
            build(&path, totals)
        })
        .collect()
}

/// Current branch of the repository a directory sits in.
///
/// Read from git rather than from the transcript: the transcript's own
/// `gitBranch` is very often the literal "HEAD" (detached, or however the
/// session was started), which is no use to anyone reading a status bar.
#[tauri::command]
pub async fn git_branch(cwd: String) -> Option<String> {
    let dir = PathBuf::from(&cwd);
    if !dir.is_dir() {
        return None;
    }
    let out = crate::modules::process::command("git")
        .args(["-C", &cwd, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        return None;
    }
    // Detached: name the commit instead, which at least identifies where you are.
    if name == "HEAD" {
        let short = crate::modules::process::command("git")
            .args(["-C", &cwd, "rev-parse", "--short", "HEAD"])
            .output()
            .ok()?;
        let sha = String::from_utf8_lossy(&short.stdout).trim().to_string();
        return (!sha.is_empty()).then(|| format!("detached at {sha}"));
    }
    Some(name)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivity {
    pub date: String,
    pub message_count: u64,
    pub session_count: u64,
    pub tool_call_count: u64,
}

#[derive(Deserialize)]
struct StatsCache {
    #[serde(rename = "lastComputedDate")]
    last_computed_date: Option<String>,
    #[serde(rename = "dailyActivity", default)]
    daily_activity: Vec<CachedDay>,
}

#[derive(Deserialize)]
struct CachedDay {
    date: String,
    #[serde(rename = "messageCount", default)]
    message_count: u64,
    #[serde(rename = "sessionCount", default)]
    session_count: u64,
    #[serde(rename = "toolCallCount", default)]
    tool_call_count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    /// The day Claude last recomputed this. Today's numbers may be missing, so
    /// the UI has to say how current the figures are rather than imply they are
    /// live — this is Claude's cache, and we only read it.
    pub last_computed_date: Option<String>,
    pub days: Vec<DailyActivity>,
}

/// Claude's own daily rollup. Absent for a user who has never run it.
#[tauri::command]
pub async fn claude_daily_stats(days: Option<usize>) -> Option<DailyStats> {
    let path = dirs::home_dir()?.join(".claude").join("stats-cache.json");
    let text = std::fs::read_to_string(path).ok()?;
    let cache: StatsCache = serde_json::from_str(&text).ok()?;
    let mut list: Vec<DailyActivity> = cache
        .daily_activity
        .into_iter()
        .map(|d| DailyActivity {
            date: d.date,
            message_count: d.message_count,
            session_count: d.session_count,
            tool_call_count: d.tool_call_count,
        })
        .collect();
    list.sort_by(|a, b| b.date.cmp(&a.date));
    list.truncate(days.unwrap_or(30));
    Some(DailyStats {
        last_computed_date: cache.last_computed_date,
        days: list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_a_working_directory() {
        assert_eq!(
            project_slug("/Users/x/Sites/kelyfos"),
            "-Users-x-Sites-kelyfos"
        );
    }

    /// Spaces and dots flatten the same way separators do.
    #[test]
    fn slugifies_awkward_directory_names() {
        assert_eq!(
            project_slug("/Users/x/Sites/photoshop plugins/PhosLut"),
            "-Users-x-Sites-photoshop-plugins-PhosLut"
        );
        assert_eq!(project_slug("/a/.hidden"), "-a--hidden");
    }

    #[test]
    fn reads_both_content_shapes() {
        assert_eq!(first_text(&serde_json::json!("plain")).as_deref(), Some("plain"));
        assert_eq!(
            first_text(&serde_json::json!([
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "asked" }
            ]))
            .as_deref(),
            Some("asked")
        );
        assert!(first_text(&serde_json::json!(42)).is_none());
    }

    #[test]
    fn truncate_keeps_short_text_whole() {
        assert_eq!(truncate("  hello\nworld ", 120), "hello world");
        assert_eq!(truncate(&"x".repeat(200), 10), "xxxxxxxxxx…");
    }

    fn totals_of(lines: &[&str]) -> Totals {
        let mut t = Totals::default();
        for l in lines {
            t.absorb(l);
        }
        t
    }

    #[test]
    fn counts_messages_and_skips_meta() {
        let t = totals_of(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-07-20T10:00:00.000Z"}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"wrapper"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5","stop_reason":"end_turn","content":[]}}"#,
        ]);
        assert_eq!(t.user_messages, 1);
        assert_eq!(t.prompts, 1);
        assert_eq!(t.assistant_messages, 1);
        assert_eq!(t.end_turns, 1);
        assert_eq!(t.first_prompt.as_deref(), Some("hi"));
        assert_eq!(t.models, vec!["claude-opus-5"]);
    }

    /// The churn numbers: only the +/- rows of a hunk are changes, context is
    /// prefixed with a space and must not be counted.
    #[test]
    fn counts_added_and_removed_lines() {
        let t = totals_of(&[r#"{"type":"user","toolUseResult":{"filePath":"/a/b.rs","structuredPatch":[{"lines":["+one","+two","-old"," context"]}]}}"#]);
        assert_eq!((t.lines_added, t.lines_removed), (2, 1));
        assert_eq!(t.files.get("/a/b.rs"), Some(&1));
    }

    #[test]
    fn sums_token_usage_including_cache() {
        let t = totals_of(&[r#"{"type":"assistant","message":{"role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":300,"cache_creation_input_tokens":40}}}"#]);
        assert_eq!(t.input_tokens, 10);
        assert_eq!(t.output_tokens, 20);
        assert_eq!(t.cache_read_tokens, 300);
        assert_eq!(t.cache_creation_tokens, 40);
    }

    #[test]
    fn counts_tools_and_their_errors() {
        let t = totals_of(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit"},{"type":"tool_use","name":"Bash"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_result","is_error":true}]}}"#,
        ]);
        assert_eq!(t.tool_calls, 2);
        assert_eq!(t.tool_errors, 1);
        assert_eq!(t.tool_names.get("Edit"), Some(&1));
    }

    #[test]
    fn reads_turn_durations_and_compactions() {
        let t = totals_of(&[
            r#"{"type":"system","subtype":"turn_duration","durationMs":1000}"#,
            r#"{"type":"system","subtype":"turn_duration","durationMs":5000}"#,
            r#"{"type":"system","subtype":"compact_boundary"}"#,
        ]);
        assert_eq!(t.turns, 2);
        assert_eq!(t.total_turn_ms, 6000);
        assert_eq!(t.longest_turn_ms, 5000);
        assert_eq!(t.compactions, 1);
    }

    #[test]
    fn tracks_first_and_last_timestamps() {
        let t = totals_of(&[
            r#"{"type":"user","timestamp":"2026-07-20T10:00:00.000Z","message":{"role":"user","content":"a"}}"#,
            r#"{"type":"user","timestamp":"2026-07-20T10:05:00.000Z","message":{"role":"user","content":"b"}}"#,
        ]);
        assert_eq!(t.updated_at.unwrap() - t.started_at.unwrap(), 5 * 60 * 1000);
    }

    /// Every tool result comes back on a line of role "user". Counting those
    /// as things the user said overstates prompts by more than tenfold on a
    /// real session, so they must not reach the prompt counter or the label.
    #[test]
    fn tool_results_are_not_prompts() {
        let t = totals_of(&[
            r#"{"type":"user","message":{"role":"user","content":"do the thing"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"and again"}]}}"#,
        ]);
        assert_eq!(t.user_messages, 3, "all three are lines of role user");
        assert_eq!(t.prompts, 2, "only two are the user talking");
        assert_eq!(t.first_prompt.as_deref(), Some("do the thing"));
    }

    /// A rejected or blocked tool call reports `toolUseResult` as a bare string
    /// rather than an object, and some tools report an array. Typed strictly,
    /// serde threw the whole line away and the message went uncounted — so the
    /// shape of one field must never decide whether a line exists.
    #[test]
    fn counts_lines_whose_tool_result_is_not_an_object() {
        let t = totals_of(&[
            r#"{"type":"user","message":{"role":"user","content":"a"},"toolUseResult":"Error: The user doesn't want to proceed with this tool use."}"#,
            r#"{"type":"user","message":{"role":"user","content":"b"},"toolUseResult":[{"type":"text","text":"x"}]}"#,
            r#"{"type":"user","message":{"role":"user","content":"c"},"toolUseResult":null}"#,
        ]);
        assert_eq!(t.user_messages, 3);
        assert_eq!(t.lines_added, 0);
        assert!(t.files.is_empty());
    }

    /// A line from a future version, or one truncated mid-write, must be
    /// skipped rather than poison the totals.
    #[test]
    fn survives_junk_lines() {
        let t = totals_of(&[
            r#"{"type":"user","message":{"role":"user","content":"a"}}"#,
            "{ half a line",
            r#"{"type":"brand_new_kind","surprise":true}"#,
        ]);
        assert_eq!(t.user_messages, 1);
    }

    /// Folding a file in two passes must give the same answer as one.
    #[test]
    fn incremental_reads_match_a_single_pass() {
        let dir = std::env::temp_dir().join(format!("kelyfos-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s1.jsonl");

        let first = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"a\"}}\n";
        let second = "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Edit\"}]}}\n";
        std::fs::write(&path, first).unwrap();
        let a = totals_for(&path).unwrap();
        assert_eq!((a.user_messages, a.tool_calls), (1, 0));

        std::fs::write(&path, format!("{first}{second}")).unwrap();
        let b = totals_for(&path).unwrap();
        assert_eq!((b.user_messages, b.tool_calls), (1, 1));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A half-written final line is read again once it is complete, not lost
    /// and not counted twice.
    #[test]
    fn waits_for_a_partial_line_to_finish() {
        let dir = std::env::temp_dir().join(format!("kelyfos-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s2.jsonl");

        std::fs::write(&path, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"con").unwrap();
        assert_eq!(totals_for(&path).unwrap().user_messages, 0);

        std::fs::write(
            &path,
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"a\"}}\n",
        )
        .unwrap();
        assert_eq!(totals_for(&path).unwrap().user_messages, 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
