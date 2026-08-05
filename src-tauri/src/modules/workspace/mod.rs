//! Workspace import/export as versioned, standalone JSON files.
//! On import, connections are matched by host+port+username and reused;
//! otherwise they are created and reported back so the UI can prompt the user.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::modules::models::*;
use crate::modules::storage::{self, Db};

pub const EXPORT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedConnection {
    name: String,
    host: String,
    port: u16,
    username: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    folder: String,
    auth_method: AuthMethod,
    #[serde(default)]
    key_path: Option<String>,
}

impl ExportedConnection {
    fn from(c: &Connection) -> Self {
        ExportedConnection {
            name: c.name.clone(),
            host: c.host.clone(),
            port: c.port,
            username: c.username.clone(),
            description: c.description.clone(),
            icon: c.icon.clone(),
            color: c.color.clone(),
            tags: c.tags.clone(),
            folder: c.folder.clone(),
            auth_method: c.auth_method,
            key_path: c.key_path.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedTerminal {
    title: String,
    /// Stable id so the exported layout JSON keeps referencing the right pane.
    id: String,
    connection: Option<ExportedConnection>,
    #[serde(default)]
    variables: std::collections::HashMap<String, String>,
    #[serde(default)]
    startup_tasks: Vec<StartupTask>,
    #[serde(default)]
    sort_order: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedWorkspace {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: String,
    /// Defaulted so exports written before tags existed still import.
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    variables: std::collections::HashMap<String, String>,
    layout: serde_json::Value,
    terminals: Vec<ExportedTerminal>,
}

#[tauri::command]
pub fn workspace_export(db: tauri::State<Db>, id: String) -> Result<String, String> {
    let workspace = {
        let list = storage::workspaces_list(db.clone())?;
        list.into_iter().find(|w| w.id == id).ok_or("workspace not found")?
    };
    let terminals = storage::terminals_list(db.clone(), id.clone())?;

    let ws_vars: std::collections::HashMap<String, String> =
        storage::variables_for(&db, "workspace", Some(&id))
            .into_iter()
            .map(|v| (v.key, v.value))
            .collect();

    let mut exported_terminals = Vec::new();
    for t in &terminals {
        let connection = match &t.connection_id {
            Some(cid) => storage::get_connection(&db, cid).ok().map(|c| ExportedConnection::from(&c)),
            None => None,
        };
        let t_vars: std::collections::HashMap<String, String> =
            storage::variables_for(&db, "terminal", Some(&t.id))
                .into_iter()
                .map(|v| (v.key, v.value))
                .collect();
        exported_terminals.push(ExportedTerminal {
            title: t.title.clone(),
            id: t.id.clone(),
            connection,
            variables: t_vars,
            startup_tasks: t.startup_tasks.clone(),
            sort_order: t.sort_order,
        });
    }

    let exported = ExportedWorkspace {
        name: workspace.name,
        description: workspace.description,
        icon: workspace.icon,
        tags: workspace.tags,
        variables: ws_vars,
        layout: workspace.layout,
        terminals: exported_terminals,
    };

    serde_json::to_string_pretty(&json!({
        "version": EXPORT_VERSION,
        "workspace": exported,
    }))
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub workspace: Workspace,
}

#[tauri::command]
pub fn workspace_import(db: tauri::State<Db>, json_text: String) -> Result<ImportResult, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(&json_text).map_err(|e| format!("invalid JSON: {}", e))?;
    let version = parsed.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if version == 0 || version > EXPORT_VERSION as u64 {
        return Err(format!("unsupported export version: {}", version));
    }
    let exported: ExportedWorkspace =
        serde_json::from_value(parsed.get("workspace").cloned().ok_or("missing workspace")?)
            .map_err(|e| format!("invalid workspace: {}", e))?;

    let existing_connections = storage::connections_list(db.clone())?;


    // Create the workspace first (empty layout; patched after terminal ids are mapped).
    let workspace = storage::workspaces_save(
        db.clone(),
        Workspace {
            id: String::new(),
            name: exported.name,
            description: exported.description,
            icon: exported.icon,
            tags: exported.tags,
            layout: serde_json::Value::Null,
            created_at: String::new(),
            updated_at: String::new(),
        },
    )?;

    for (k, v) in exported.variables {
        storage::variables_set(
            db.clone(),
            Variable {
                id: String::new(),
                scope: "workspace".into(),
                owner_id: Some(workspace.id.clone()),
                key: k,
                value: v,
            },
        )?;
    }

    let mut layout_json = serde_json::to_string(&exported.layout).unwrap_or_default();

    for et in exported.terminals {
        let connection_id = match &et.connection {
            Some(ec) => {
                let reuse = existing_connections.iter().find(|c| {
                    c.host == ec.host && c.port == ec.port && c.username == ec.username
                });
                match reuse {
                    Some(c) => Some(c.id.clone()),
                    None => {
                        let created = storage::connections_save(
                            db.clone(),
                            Connection {
                                id: String::new(),
                                name: ec.name.clone(),
                                host: ec.host.clone(),
                                port: ec.port,
                                username: ec.username.clone(),
                                description: ec.description.clone(),
                                icon: ec.icon.clone(),
                                color: ec.color.clone(),
                                tags: ec.tags.clone(),
                                folder: ec.folder.clone(),
                                auth_method: ec.auth_method,
                                key_path: ec.key_path.clone(),
                                created_at: String::new(),
                                updated_at: String::new(),
                            },
                        )?;
                        Some(created.id)
                    }
                }
            }
            None => None,
        };

        let terminal = storage::terminals_save(
            db.clone(),
            TerminalDef {
                id: String::new(),
                workspace_id: workspace.id.clone(),
                connection_id,
                title: et.title,
                startup_tasks: et.startup_tasks,
                sort_order: et.sort_order,
                // Not exported: a shell path is machine-specific, so an
                // imported terminal falls back to the global shell setting.
                shell: String::new(),
            },
        )?;

        // Remap the old terminal id inside the layout JSON to the new one.
        layout_json = layout_json.replace(&et.id, &terminal.id);

        for (k, v) in et.variables {
            storage::variables_set(
                db.clone(),
                Variable {
                    id: String::new(),
                    scope: "terminal".into(),
                    owner_id: Some(terminal.id.clone()),
                    key: k,
                    value: v,
                },
            )?;
        }
    }

    let mut workspace = workspace;
    workspace.layout = serde_json::from_str(&layout_json).unwrap_or(serde_json::Value::Null);
    let workspace = storage::workspaces_save(db.clone(), workspace)?;

    Ok(ImportResult { workspace })
}

#[tauri::command]
pub fn workspace_duplicate(db: tauri::State<Db>, id: String) -> Result<Workspace, String> {
    let exported = workspace_export(db.clone(), id)?;
    let mut result = workspace_import(db.clone(), exported)?;
    result.workspace.name = format!("{} (copy)", result.workspace.name);
    storage::workspaces_save(db, result.workspace)
}
