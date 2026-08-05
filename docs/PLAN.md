# Kelyfos — Implementation Plan

A local-first SSH workspace manager. Tauri 2 + Rust backend, React + TypeScript + Zustand frontend, xterm.js terminals, FlexLayout docking, SQLite storage, OS credential store (Windows Credential Manager / macOS Keychain via `keyring`).

---

## 1. Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        Tauri 2 WebView (Frontend)                │
│                                                                  │
│  React + TypeScript + Zustand                                    │
│  ┌────────────┐ ┌──────────────┐ ┌───────────────────────────┐  │
│  │  Sidebar   │ │ Workspace    │ │  FlexLayout Grid          │  │
│  │ Workspaces │ │ Tabs +       │ │  ┌─────────┐ ┌─────────┐  │  │
│  │ Connections│ │ Status Badges│ │  │ xterm.js│ │ xterm.js│  │  │
│  └────────────┘ └──────────────┘ │  └─────────┘ └─────────┘  │  │
│                                  └───────────────────────────┘  │
│  Zustand stores: connections / workspaces / terminals /         │
│                  layout / settings / themes                     │
└───────────────┬────────────────────────────▲─────────────────────┘
                │ invoke (commands)          │ events (channels)
                │ ssh_open, ssh_write, …     │ ssh:data, ssh:status
┌───────────────▼────────────────────────────┴─────────────────────┐
│                         Rust Backend (Tauri core)                │
│                                                                  │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌──────────────────┐ │
│  │ ssh       │ │ storage   │ │ secrets   │ │ workspace        │ │
│  │ russh     │ │ SQLite    │ │ keyring   │ │ import/export    │ │
│  │ sessions  │ │ (rusqlite)│ │ (WCM/KC)  │ │ JSON files       │ │
│  └───────────┘ └───────────┘ └───────────┘ └──────────────────┘ │
│  ┌───────────┐ ┌───────────┐ ┌───────────────────────────────┐  │
│  │ variables │ │ startup   │ │ future: ai / mcp (stub mods)  │  │
│  │ resolver  │ │ task eng. │ └───────────────────────────────┘  │
│  └───────────┘ └───────────┘                                    │
└──────────────────────────────────────────────────────────────────┘
        │                     │
   <data dir>/kelyfos/       OS Credential Store
   app.db (SQLite)            (passwords, passphrases — never in DB)
```

Data flow for a terminal session:
1. Frontend `invoke("ssh_open", { terminalId, connectionId, cols, rows })`
2. Rust loads connection from SQLite, fetches secret from keyring, opens SSH session + PTY channel (russh)
3. Rust streams output via Tauri event `ssh:data:{terminalId}`; status changes via `ssh:status:{terminalId}`
4. Frontend writes keystrokes with `ssh_write`; resizes with `ssh_resize`
5. Startup task engine runs task list against the channel after connect (wait_for_prompt / command / wait)

---

## 2. Database Schema (SQLite)

```sql
CREATE TABLE connections (
  id          TEXT PRIMARY KEY,          -- uuid v4
  name        TEXT NOT NULL,
  host        TEXT NOT NULL,
  port        INTEGER NOT NULL DEFAULT 22,
  username    TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  icon        TEXT NOT NULL DEFAULT 'server',
  color       TEXT NOT NULL DEFAULT '#6ea8fe',
  tags        TEXT NOT NULL DEFAULT '[]',        -- JSON array
  folder      TEXT NOT NULL DEFAULT '',
  auth_method TEXT NOT NULL DEFAULT 'password',  -- 'password' | 'key'
  credential_id  TEXT,                           -- keyring entry id (password)
  key_path       TEXT,                           -- private key path (key auth)
  passphrase_credential_id TEXT,                 -- keyring entry id (passphrase)
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

CREATE TABLE workspaces (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  icon        TEXT NOT NULL DEFAULT 'layout',
  layout      TEXT NOT NULL DEFAULT '{}',   -- FlexLayout JSON model
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

CREATE TABLE terminals (
  id            TEXT PRIMARY KEY,
  workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
  title         TEXT NOT NULL DEFAULT '',
  startup_tasks TEXT NOT NULL DEFAULT '[]', -- JSON task list
  sort_order    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE variables (
  id        TEXT PRIMARY KEY,
  scope     TEXT NOT NULL,   -- 'global' | 'workspace' | 'terminal'
  owner_id  TEXT,            -- workspace_id or terminal_id; NULL for global
  key       TEXT NOT NULL,
  value     TEXT NOT NULL,
  UNIQUE(scope, owner_id, key)
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,    -- 'theme', 'terminal.fontSize', 'ui.density', …
  value TEXT NOT NULL        -- JSON
);

CREATE TABLE app_state (          -- session restoration
  key   TEXT PRIMARY KEY,         -- 'open_workspaces', 'active_workspace'
  value TEXT NOT NULL
);
```

Never stored in SQLite: passwords, passphrases. Only keyring credential IDs.

---

## 3. Folder Structure

```
kelyfos/
├── docs/                      # this plan + schemas
├── src/                       # React frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   │   ├── sidebar/           # WorkspaceList, ConnectionList, folders, search
│   │   ├── tabs/              # WorkspaceTabs, StatusBadge
│   │   ├── layout/            # FlexLayout host, TerminalPane
│   │   ├── terminal/          # XtermView (xterm.js wrapper), SearchBar
│   │   ├── dialogs/           # ConnectionDialog, WorkspaceDialog, VariablesDialog,
│   │   │                      #   StartupTasksEditor, ImportExport, Settings
│   │   └── common/            # Button, Input, Modal, ContextMenu, …
│   ├── stores/                # Zustand: connections, workspaces, terminals,
│   │                          #   layout, settings, statuses
│   ├── lib/                   # ipc.ts (typed invoke wrappers), variables.ts,
│   │                          #   themes.ts, types.ts
│   └── styles/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs / lib.rs
│   │   └── modules/
│   │       ├── ssh/           # session manager, russh client, PTY channel
│   │       ├── storage/       # SQLite (rusqlite), migrations, repos
│   │       ├── secrets/       # keyring wrapper
│   │       ├── workspace/     # import/export, .workspace.json
│   │       ├── variables/     # scope resolution
│   │       ├── startup/       # task engine
│   │       ├── settings/
│   │       ├── ai/            # stub — future
│   │       └── mcp/           # stub — future
│   ├── Cargo.toml
│   └── tauri.conf.json
└── package.json
```

---

## 4. Rust Modules

| Module | Responsibility | Key crates |
|---|---|---|
| `ssh` | Session lifecycle, PTY channels, data streaming, reconnect | `russh`, `russh-keys`, `tokio` |
| `storage` | SQLite open/migrate, CRUD repos for all tables | `rusqlite` |
| `secrets` | set/get/delete secrets by credential ID | `keyring` (WCM on Windows, Keychain on macOS) |
| `workspace` | Workspace JSON export/import (versioned, no secrets) | `serde_json` |
| `variables` | Merge global → workspace → terminal; `${VAR}` substitution | — |
| `startup` | Task engine: `wait_for_prompt`, `command`, `wait`; extensible enum | `tokio` |
| `settings` | Settings/app_state key-value access | — |
| `ai`, `mcp` | Empty stubs with trait definitions for Phase 2 | — |

SSH state: `SessionManager` (in Tauri managed state) holds `HashMap<TerminalId, SshSession>`; each session runs a tokio task pumping channel output to a Tauri event.

---

## 5. React Component Tree

```
App
├── Sidebar
│   ├── SidebarSection(Workspaces) → WorkspaceItem (ctx menu: open/rename/duplicate/delete/export)
│   ├── SidebarSection(Connections) → FolderGroup → ConnectionItem (ctx menu)
│   └── SearchInput
├── MainArea
│   ├── WorkspaceTabs → WorkspaceTab (StatusBadge: connected/connecting/disconnected/error/…)
│   └── WorkspaceView (per open workspace)
│       └── FlexLayoutHost (FlexLayout Model ↔ store)
│           └── TerminalPane
│               ├── PaneHeader (title, status, reconnect)
│               ├── XtermView (xterm + fit + search + webgl addons)
│               └── TerminalSearchBar
└── Dialogs (portal): ConnectionDialog, WorkspaceDialog, VariablesEditor,
                      StartupTasksEditor, SettingsDialog, ImportExportDialog
```

---

## 6. IPC API (Tauri commands)

```
Connections:  connections_list / save / delete
Secrets:      secret_set(credentialId, value) / secret_exists / secret_delete
Workspaces:   workspaces_list / save / delete / duplicate
              workspace_export(id) -> JSON string
              workspace_import(json) -> { workspace, missingCredentials[] }
Terminals:    terminals_list(workspaceId) / save / delete
Variables:    variables_list(scope, ownerId) / set / delete
              variables_resolve(workspaceId, terminalId) -> merged map
SSH:          ssh_open(terminalId, connectionId, cols, rows)
              ssh_write(terminalId, data)
              ssh_resize(terminalId, cols, rows)
              ssh_close(terminalId)
Settings:     settings_get_all / settings_set(key, value)
AppState:     state_get(key) / state_set(key, value)

Events (Rust → JS):
  ssh:data:{terminalId}    (base64 bytes)
  ssh:status:{terminalId}  ('connecting'|'connected'|'disconnected'|'error', message?)
```

Status enum is a string union — extensible for `ai_running`, `waiting_approval` in Phase 2.

---

## 7. State Management (Zustand)

- `useConnectionsStore` — list, CRUD via IPC, search/tags/folders filter
- `useWorkspacesStore` — list, open workspace ids, active id; persists open/active to `app_state`
- `useTerminalsStore` — terminal defs per workspace; runtime status map `{terminalId: Status}`
- `useLayoutStore` — FlexLayout model JSON per workspace; debounced save on change
- `useSettingsStore` — theme, terminal appearance, density; hydrated at boot
- Boot sequence: hydrate settings → connections → workspaces → restore open tabs/active → mount layouts → terminals auto-connect (restoration)

---

## 8. Workspace JSON Schema (export format)

```json
{
  "version": 1,
  "workspace": {
    "name": "Customer A Production",
    "description": "",
    "icon": "layout",
    "variables": { "CLIENT": "Acme", "PROJECT_DIR": "/srv/acme" },
    "layout": { "...FlexLayout model..." },
    "terminals": [
      {
        "ref": "t1",
        "title": "App ${ENV}",
        "connection": {
          "name": "Production Server", "host": "1.2.3.4", "port": 22,
          "username": "deploy", "authMethod": "password"
        },
        "variables": { "SERVICE": "frontend" },
        "startupTasks": [
          { "type": "wait_for_prompt" },
          { "type": "command", "value": "cd ${PROJECT_DIR}" },
          { "type": "wait", "milliseconds": 2000 }
        ]
      }
    ]
  }
}
```

Secrets are never exported. On import, connections are matched by host+port+username (reuse) or created; missing credentials prompt the user.

---

## 9. Implementation Roadmap

### Phase 1 — MVP
| Step | Deliverable |
|---|---|
| 1.1 | Scaffold Tauri 2 + React + TS + Vite; dark shell UI (sidebar, tabs, main area) |
| 1.2 | Rust: storage module (SQLite + migrations), settings, secrets (keyring) |
| 1.3 | Connections CRUD end-to-end (dialog, list, search, tags, folders, secrets) |
| 1.4 | SSH module (russh): open/write/resize/close, status events, reconnect |
| 1.5 | xterm.js terminal pane wired to SSH (copy/paste, scrollback, search, resize) |
| 1.6 | Workspaces CRUD + workspace tabs + statuses |
| 1.7 | FlexLayout grid: split/drag/tabs/nested; serialize + restore per workspace |
| 1.8 | Variable system (3 scopes, resolver, `${}` substitution, editor UI) |
| 1.9 | Startup task engine (wait_for_prompt, command, wait) + editor UI |
| 1.10 | Import/export (connections, workspaces, settings; `.workspace.json` files) |
| 1.11 | Themes (built-in dark themes, terminal + UI customization) |
| 1.12 | State restoration (open workspaces, active, layouts, selected pane) |

### Phase 2 — AI groundwork (later)
AI agent stubs, chat panel, MCP server module, per-workspace permission system, image paste.

Security invariants (enforced from step 1.2): secrets only in OS credential store; no secrets in exports or logs; no network calls except user SSH sessions.
