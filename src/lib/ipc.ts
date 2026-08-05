import { invoke } from "@tauri-apps/api/core";
import type {
  AgentSnapshot,
  ClaudeHealth,
  ClaudeTestResult,
  Connection,
  DailyStats,
  ImportResult,
  Preset,
  SessionStats,
  TerminalDef,
  Variable,
  VariableScope,
  Workspace,
} from "./types";

export const ipc = {
  // Connections
  connectionsList: () => invoke<Connection[]>("connections_list"),
  connectionsSave: (connection: Connection) =>
    invoke<Connection>("connections_save", { connection }),
  connectionsDelete: (id: string) => invoke<void>("connections_delete", { id }),

  // Workspaces
  workspacesList: () => invoke<Workspace[]>("workspaces_list"),
  workspacesSave: (workspace: Workspace) =>
    invoke<Workspace>("workspaces_save", { workspace }),
  workspacesDelete: (id: string) => invoke<void>("workspaces_delete", { id }),
  workspaceDuplicate: (id: string) =>
    invoke<Workspace>("workspace_duplicate", { id }),
  workspaceExport: (id: string) => invoke<string>("workspace_export", { id }),
  workspaceImport: (jsonText: string) =>
    invoke<ImportResult>("workspace_import", { jsonText }),

  // Terminals
  terminalsList: (workspaceId: string) =>
    invoke<TerminalDef[]>("terminals_list", { workspaceId }),
  terminalsSave: (terminal: TerminalDef) =>
    invoke<TerminalDef>("terminals_save", { terminal }),
  terminalsDelete: (id: string) => invoke<void>("terminals_delete", { id }),

  // Presets
  presetsList: () => invoke<Preset[]>("presets_list"),
  presetsSave: (preset: Preset) => invoke<Preset>("presets_save", { preset }),
  presetsDelete: (id: string) => invoke<void>("presets_delete", { id }),
  /** Snapshots a preset's variables onto a newly created terminal. */
  presetCopyVariables: (presetId: string, terminalId: string) =>
    invoke<void>("preset_copy_variables", { presetId, terminalId }),

  // Variables
  variablesList: (scope: VariableScope, ownerId: string | null) =>
    invoke<Variable[]>("variables_list", { scope, ownerId }),
  variablesSet: (variable: Variable) =>
    invoke<Variable>("variables_set", { variable }),
  variablesDelete: (id: string) => invoke<void>("variables_delete", { id }),
  variablesResolve: (workspaceId: string | null, terminalId: string | null) =>
    invoke<Record<string, string>>("variables_resolve", {
      workspaceId,
      terminalId,
    }),

  // Settings & app state
  settingsGetAll: () => invoke<Record<string, unknown>>("settings_get_all"),
  settingsSet: (key: string, value: unknown) =>
    invoke<void>("settings_set", { key, value }),
  stateGet: <T>(key: string) => invoke<T | null>("state_get", { key }),
  stateSet: (key: string, value: unknown) =>
    invoke<void>("state_set", { key, value }),

  // Local PTY — the primary session backend. A saved connection does not get
  // its own transport; an `ssh …` line is typed into the shell instead.
  ptyOpen: (
    terminalId: string,
    connectionId: string | null,
    workspaceId: string | null,
    shell: string | null,
    cwd: string | null,
    cols: number,
    rows: number
  ) =>
    invoke<void>("pty_open", {
      terminalId,
      connectionId,
      workspaceId,
      shell,
      cwd,
      cols,
      rows,
    }),
  ptyWrite: (terminalId: string, data: string) =>
    invoke<void>("pty_write", { terminalId, data }),
  ptyResize: (terminalId: string, cols: number, rows: number) =>
    invoke<void>("pty_resize", { terminalId, cols, rows }),
  ptyClose: (terminalId: string) => invoke<void>("pty_close", { terminalId }),
  /** Working directory of a live terminal, or null if it can't be read. */
  ptyCwd: (terminalId: string) =>
    invoke<string | null>("pty_cwd", { terminalId }),
  shellsList: () => invoke<string[]>("shells_list"),
  fontsList: () =>
    invoke<{ monospace: string[]; all: string[] }>("fonts_list"),

  /** Database snapshot taken before an update, so a bad release can be undone.
   *  `VACUUM INTO` on the backend, so the file stands alone — no -wal/-shm
   *  sidecars to keep with it. Not user-facing; the JSON export is. */
  backupCreate: (label: string | null) =>
    invoke<string>("backup_create", { label }),

  // Claude Code integration. Every one of these is safe to call on a machine
  // without Claude installed — they report its absence rather than failing.
  claudeHealth: () => invoke<ClaudeHealth>("claude_health"),
  /** Installs the hooks in ~/.claude/settings.json. User-initiated only. */
  claudeConnect: () => invoke<void>("claude_connect"),
  claudeDisconnect: () => invoke<void>("claude_disconnect"),
  /** Writes the Kelyfos theme file into ~/.claude/themes. Never selects it —
   *  the user does that with /theme inside Claude. */
  claudeThemeInstall: () => invoke<void>("claude_theme_install"),
  claudeThemeRemove: () => invoke<void>("claude_theme_remove"),
  claudeStates: () => invoke<AgentSnapshot[]>("claude_states"),
  /** Runs the hook script for real and reports whether it reached the app. */
  claudeTest: () => invoke<ClaudeTestResult>("claude_test"),
  claudeSessions: (cwd: string, limit?: number) =>
    invoke<SessionStats[]>("claude_sessions", { cwd, limit: limit ?? null }),
  /** Full stats for one transcript. Parsed incrementally on the backend, so
   *  polling this every few seconds costs only the bytes newly appended. */
  claudeSessionStats: (path: string) =>
    invoke<SessionStats | null>("claude_session_stats", { path }),
  /** Current branch of the repo a directory sits in, read from git itself. */
  gitBranch: (cwd: string) => invoke<string | null>("git_branch", { cwd }),
  claudeDailyStats: (days?: number) =>
    invoke<DailyStats | null>("claude_daily_stats", { days: days ?? null }),
};
