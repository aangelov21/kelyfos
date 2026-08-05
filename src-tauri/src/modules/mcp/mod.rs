//! Phase 2: MCP server. Architecture placeholder only — no implementation.
//!
//! Planned tools: list workspaces, read workspace variables, read terminal
//! output, open terminal, execute command, read/write files.
//! Permissions are granted per workspace (e.g. production: read-only).

#![allow(dead_code)]

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPermission {
    ListWorkspaces,
    ReadVariables,
    ReadTerminal,
    OpenTerminal,
    ExecuteCommand,
    ReadFiles,
    WriteFiles,
}

/// Per-workspace permission grants; deny by default.
pub struct WorkspacePermissions {
    pub workspace_id: String,
    pub allowed: Vec<McpPermission>,
}
