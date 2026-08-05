pub mod migrations;

use rusqlite::{params, Connection as SqlConnection};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::modules::models::*;

pub struct Db(pub Mutex<SqlConnection>);

fn db_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kelyfos");
    std::fs::create_dir_all(&dir).ok();
    dir.join("app.db")
}

/// Opens the database and brings it to the schema this build expects. The
/// error is for the user to read: a migration that fails leaves the app with
/// no usable storage, and a message naming the backup beats a crash at launch.
pub fn init() -> Result<Db, String> {
    let path = db_path();
    let mut conn = SqlConnection::open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;

    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|e| format!("cannot configure database: {e}"))?;

    migrations::apply(&mut conn, &|c| {
        crate::modules::backup::create_from_conn(c, Some("before-migration"))
    })?;

    Ok(Db(Mutex::new(conn)))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn row_to_connection(row: &rusqlite::Row) -> rusqlite::Result<Connection> {
    Ok(Connection {
        id: row.get("id")?,
        name: row.get("name")?,
        host: row.get("host")?,
        port: row.get::<_, i64>("port")? as u16,
        username: row.get("username")?,
        description: row.get("description")?,
        icon: row.get("icon")?,
        color: row.get("color")?,
        tags: serde_json::from_str(&row.get::<_, String>("tags")?).unwrap_or_default(),
        folder: row.get("folder")?,
        auth_method: AuthMethod::from_str(&row.get::<_, String>("auth_method")?),
        key_path: row.get("key_path")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Read one settings row from Rust. The frontend owns the settings store, but
/// the PTY layer needs a few of them before any window is talking to it.
pub fn get_setting(db: &Db, key: &str) -> Option<serde_json::Value> {
    let conn = db.0.lock().unwrap();
    let raw: String = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn get_connection(db: &Db, id: &str) -> Result<Connection, String> {
    let conn = db.0.lock().unwrap();
    conn.query_row("SELECT * FROM connections WHERE id = ?1", params![id], row_to_connection)
        .map_err(|e| e.to_string())
}

// ---------- Connections ----------

#[tauri::command]
pub fn connections_list(db: tauri::State<Db>) -> Result<Vec<Connection>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT * FROM connections ORDER BY folder, name COLLATE NOCASE")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_connection)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn connections_save(db: tauri::State<Db>, mut connection: Connection) -> Result<Connection, String> {
    let conn = db.0.lock().unwrap();
    if connection.id.is_empty() {
        connection.id = uuid::Uuid::new_v4().to_string();
        connection.created_at = now();
    }
    connection.updated_at = now();
    if connection.created_at.is_empty() {
        connection.created_at = connection.updated_at.clone();
    }
    conn.execute(
        r#"INSERT INTO connections
            (id, name, host, port, username, description, icon, color, tags, folder,
             auth_method, key_path, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
           ON CONFLICT(id) DO UPDATE SET
             name=?2, host=?3, port=?4, username=?5, description=?6, icon=?7, color=?8,
             tags=?9, folder=?10, auth_method=?11, key_path=?12, updated_at=?14"#,
        params![
            connection.id,
            connection.name,
            connection.host,
            connection.port as i64,
            connection.username,
            connection.description,
            connection.icon,
            connection.color,
            serde_json::to_string(&connection.tags).unwrap(),
            connection.folder,
            connection.auth_method.as_str(),
            connection.key_path,
            connection.created_at,
            connection.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(connection)
}

#[tauri::command]
pub fn connections_delete(db: tauri::State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    // A preset built on this connection has nothing left to open, so it goes
    // too (with its variables). Terminals keep their FK's SET NULL instead —
    // one already on screen must not disappear from under the user.
    conn.execute(
        "DELETE FROM variables WHERE scope='preset' AND owner_id IN
           (SELECT id FROM presets WHERE connection_id = ?1)",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM presets WHERE connection_id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM connections WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- Workspaces ----------

fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        icon: row.get("icon")?,
        tags: serde_json::from_str(&row.get::<_, String>("tags")?).unwrap_or_default(),
        layout: serde_json::from_str(&row.get::<_, String>("layout")?)
            .unwrap_or(serde_json::Value::Null),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[tauri::command]
pub fn workspaces_list(db: tauri::State<Db>) -> Result<Vec<Workspace>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT * FROM workspaces ORDER BY name COLLATE NOCASE")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_workspace).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspaces_save(db: tauri::State<Db>, mut workspace: Workspace) -> Result<Workspace, String> {
    let conn = db.0.lock().unwrap();
    if workspace.id.is_empty() {
        workspace.id = uuid::Uuid::new_v4().to_string();
        workspace.created_at = now();
    }
    workspace.updated_at = now();
    if workspace.created_at.is_empty() {
        workspace.created_at = workspace.updated_at.clone();
    }
    conn.execute(
        r#"INSERT INTO workspaces (id, name, description, icon, tags, layout, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
           ON CONFLICT(id) DO UPDATE SET
             name=?2, description=?3, icon=?4, tags=?5, layout=?6, updated_at=?8"#,
        params![
            workspace.id,
            workspace.name,
            workspace.description,
            workspace.icon,
            serde_json::to_string(&workspace.tags).unwrap(),
            serde_json::to_string(&workspace.layout).unwrap(),
            workspace.created_at,
            workspace.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(workspace)
}

#[tauri::command]
pub fn workspaces_delete(db: tauri::State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM variables WHERE scope='workspace' AND owner_id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- Terminals ----------

fn row_to_terminal(row: &rusqlite::Row) -> rusqlite::Result<TerminalDef> {
    Ok(TerminalDef {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        connection_id: row.get("connection_id")?,
        title: row.get("title")?,
        startup_tasks: serde_json::from_str(&row.get::<_, String>("startup_tasks")?)
            .unwrap_or_default(),
        sort_order: row.get("sort_order")?,
        shell: row.get("shell")?,
    })
}

#[tauri::command]
pub fn terminals_list(db: tauri::State<Db>, workspace_id: String) -> Result<Vec<TerminalDef>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT * FROM terminals WHERE workspace_id = ?1 ORDER BY sort_order")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![workspace_id], row_to_terminal)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn get_terminal(db: &Db, id: &str) -> Result<TerminalDef, String> {
    let conn = db.0.lock().unwrap();
    conn.query_row("SELECT * FROM terminals WHERE id = ?1", params![id], row_to_terminal)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn terminals_save(db: tauri::State<Db>, mut terminal: TerminalDef) -> Result<TerminalDef, String> {
    let conn = db.0.lock().unwrap();
    if terminal.id.is_empty() {
        terminal.id = uuid::Uuid::new_v4().to_string();
    }
    conn.execute(
        r#"INSERT INTO terminals (id, workspace_id, connection_id, title, startup_tasks, sort_order, shell)
           VALUES (?1,?2,?3,?4,?5,?6,?7)
           ON CONFLICT(id) DO UPDATE SET
             workspace_id=?2, connection_id=?3, title=?4, startup_tasks=?5, sort_order=?6, shell=?7"#,
        params![
            terminal.id,
            terminal.workspace_id,
            terminal.connection_id,
            terminal.title,
            serde_json::to_string(&terminal.startup_tasks).unwrap(),
            terminal.sort_order,
            terminal.shell,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(terminal)
}

#[tauri::command]
pub fn terminals_delete(db: tauri::State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM terminals WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM variables WHERE scope='terminal' AND owner_id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- Presets ----------

fn row_to_preset(row: &rusqlite::Row) -> rusqlite::Result<Preset> {
    Ok(Preset {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        connection_id: row.get("connection_id")?,
        startup_tasks: serde_json::from_str(&row.get::<_, String>("startup_tasks")?)
            .unwrap_or_default(),
        shell: row.get("shell")?,
        color: row.get("color")?,
        tags: serde_json::from_str(&row.get::<_, String>("tags")?).unwrap_or_default(),
        sort_order: row.get("sort_order")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[tauri::command]
pub fn presets_list(db: tauri::State<Db>) -> Result<Vec<Preset>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT * FROM presets ORDER BY sort_order, name")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_preset).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn presets_save(db: tauri::State<Db>, mut preset: Preset) -> Result<Preset, String> {
    let conn = db.0.lock().unwrap();
    if preset.id.is_empty() {
        preset.id = uuid::Uuid::new_v4().to_string();
    }
    if preset.created_at.is_empty() {
        preset.created_at = now();
    }
    preset.updated_at = now();
    conn.execute(
        r#"INSERT INTO presets (id, name, description, connection_id, startup_tasks, shell, color, tags, sort_order, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
           ON CONFLICT(id) DO UPDATE SET
             name=?2, description=?3, connection_id=?4, startup_tasks=?5, shell=?6,
             color=?7, tags=?8, sort_order=?9, updated_at=?11"#,
        params![
            preset.id,
            preset.name,
            preset.description,
            preset.connection_id,
            serde_json::to_string(&preset.startup_tasks).unwrap(),
            preset.shell,
            preset.color,
            serde_json::to_string(&preset.tags).unwrap(),
            preset.sort_order,
            preset.created_at,
            preset.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(preset)
}

#[tauri::command]
pub fn presets_delete(db: tauri::State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM presets WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM variables WHERE scope='preset' AND owner_id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Copies a preset's variables onto a freshly created terminal. Part of the
/// snapshot: the terminal owns them afterwards, so editing either side is safe.
#[tauri::command]
pub fn preset_copy_variables(
    db: tauri::State<Db>,
    preset_id: String,
    terminal_id: String,
) -> Result<(), String> {
    let vars = variables_for(&db, "preset", Some(&preset_id));
    let conn = db.0.lock().unwrap();
    for v in vars {
        conn.execute(
            r#"INSERT INTO variables (id, scope, owner_id, key, value)
               VALUES (?1,'terminal',?2,?3,?4)
               ON CONFLICT(scope, owner_id, key) DO UPDATE SET value=?4"#,
            params![uuid::Uuid::new_v4().to_string(), terminal_id, v.key, v.value],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- Variables ----------

fn row_to_variable(row: &rusqlite::Row) -> rusqlite::Result<Variable> {
    Ok(Variable {
        id: row.get("id")?,
        scope: row.get("scope")?,
        owner_id: row.get("owner_id")?,
        key: row.get("key")?,
        value: row.get("value")?,
    })
}

#[tauri::command]
pub fn variables_list(
    db: tauri::State<Db>,
    scope: String,
    owner_id: Option<String>,
) -> Result<Vec<Variable>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT * FROM variables WHERE scope = ?1 AND (owner_id = ?2 OR (?2 IS NULL AND owner_id IS NULL)) ORDER BY key")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![scope, owner_id], row_to_variable)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn variables_set(db: tauri::State<Db>, mut variable: Variable) -> Result<Variable, String> {
    let conn = db.0.lock().unwrap();
    if variable.id.is_empty() {
        variable.id = uuid::Uuid::new_v4().to_string();
    }
    conn.execute(
        r#"INSERT INTO variables (id, scope, owner_id, key, value)
           VALUES (?1,?2,?3,?4,?5)
           ON CONFLICT(scope, owner_id, key) DO UPDATE SET value=?5"#,
        params![variable.id, variable.scope, variable.owner_id, variable.key, variable.value],
    )
    .map_err(|e| e.to_string())?;
    Ok(variable)
}

#[tauri::command]
pub fn variables_delete(db: tauri::State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM variables WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn variables_for(db: &Db, scope: &str, owner_id: Option<&str>) -> Vec<Variable> {
    let conn = db.0.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT * FROM variables WHERE scope = ?1 AND (owner_id = ?2 OR (?2 IS NULL AND owner_id IS NULL))",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params![scope, owner_id], row_to_variable)
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

// ---------- Settings & app state ----------

#[tauri::command]
pub fn settings_get_all(db: tauri::State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT key, value FROM settings").map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        map.insert(row.0, serde_json::from_str(&row.1).unwrap_or(serde_json::Value::Null));
    }
    Ok(serde_json::Value::Object(map))
}

#[tauri::command]
pub fn settings_set(db: tauri::State<Db>, key: String, value: serde_json::Value) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2",
        params![key, serde_json::to_string(&value).unwrap()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn state_get(db: tauri::State<Db>, key: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let result: Result<String, _> = conn.query_row(
        "SELECT value FROM app_state WHERE key = ?1",
        params![key],
        |row| row.get(0),
    );
    match result {
        Ok(v) => Ok(serde_json::from_str(&v).ok()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn state_set(db: tauri::State<Db>, key: String, value: serde_json::Value) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO app_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2",
        params![key, serde_json::to_string(&value).unwrap()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
