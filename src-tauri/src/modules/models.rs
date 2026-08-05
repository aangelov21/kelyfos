use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub folder: String,
    pub auth_method: AuthMethod,
    /// Path to a private key, passed to `ssh -i`.
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Password,
    Key,
}

impl AuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::Key => "key",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "key" => AuthMethod::Key,
            _ => AuthMethod::Password,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// FlexLayout model JSON, stored verbatim.
    #[serde(default)]
    pub layout: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDef {
    pub id: String,
    pub workspace_id: String,
    pub connection_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub startup_tasks: Vec<StartupTask>,
    #[serde(default)]
    pub sort_order: i64,
    /// Per-terminal shell override; empty falls back to the global setting.
    #[serde(default)]
    pub shell: String,
}

/// A reusable recipe for a terminal: which connection to use, the shell, the
/// startup tasks, plus its own variables (scope `preset`). Opening one copies
/// everything into a fresh terminal — the two are independent from then on, so
/// editing a preset never disturbs terminals already on screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub startup_tasks: Vec<StartupTask>,
    /// Shell override; empty falls back to the global setting.
    #[serde(default)]
    pub shell: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// Startup task engine steps. Extensible: add variants for
/// confirmation, conditional, open_terminal, notification, ai in Phase 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StartupTask {
    WaitForPrompt,
    Command { value: String },
    Wait { milliseconds: u64 },
}

/// Commands sent to a live terminal session's driver task. Shared by both
/// session backends (`ssh`, `pty`) so the startup engine can drive either.
pub enum SessionCmd {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub id: String,
    pub scope: String, // global | workspace | terminal
    pub owner_id: Option<String>,
    pub key: String,
    pub value: String,
}
