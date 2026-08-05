export type AuthMethod = "password" | "key";

export interface Connection {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  description: string;
  icon: string;
  /** @deprecated Records no longer carry a colour — tags do. The column stays
   *  so the backend keeps round-tripping saved rows unchanged. */
  color: string;
  tags: string[];
  folder: string;
  authMethod: AuthMethod;
  /** Path to a private key, passed to `ssh -i`. */
  keyPath: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Workspace {
  id: string;
  name: string;
  description: string;
  icon: string;
  tags: string[];
  layout: unknown; // FlexLayout model JSON
  createdAt: string;
  updatedAt: string;
}

export type StartupTask =
  | { type: "wait_for_prompt" }
  | { type: "command"; value: string }
  | { type: "wait"; milliseconds: number };

export interface TerminalDef {
  id: string;
  workspaceId: string;
  connectionId: string | null;
  title: string;
  startupTasks: StartupTask[];
  sortOrder: number;
  /** Per-terminal shell override; empty falls back to the global setting. */
  shell: string;
}

/**
 * A reusable recipe for a terminal. Opening one copies its connection, shell,
 * startup tasks and variables into a new terminal, which is independent from
 * then on — editing a preset never changes terminals already open.
 */
export interface Preset {
  id: string;
  name: string;
  description: string;
  connectionId: string | null;
  startupTasks: StartupTask[];
  /** Shell override; empty falls back to the global setting. */
  shell: string;
  /** @deprecated See Connection.color. */
  color: string;
  tags: string[];
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export type VariableScope = "global" | "workspace" | "terminal" | "preset";

export interface Variable {
  id: string;
  scope: VariableScope;
  ownerId: string | null;
  key: string;
  value: string;
}

export type TerminalStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "busy"
  | "disconnected"
  | "error"
  /** Claude Code is working. */
  | "ai_running"
  /** Claude Code is blocked on the user — a permission prompt or a question. */
  | "ai_waiting"
  /** Claude Code finished a run the user has not looked at yet. */
  | "ai_done"
  /** Printed something while nobody was looking at it. */
  | "activity";

/** What Claude Code is doing inside one terminal, as reported by its hooks. */
export type AgentPhase = "idle" | "running" | "waiting";

export interface AgentSnapshot {
  terminalId: string;
  /** A claude process is running here. Detected without any configuration. */
  present: boolean;
  phase: AgentPhase;
  sessionId: string | null;
  transcriptPath: string | null;
  cwd: string | null;
  /** Tool currently running, when one is. */
  tool: string | null;
  lastToolError: boolean;
}

/** Each link in the Claude bridge, reported separately: a break anywhere in the
 *  chain otherwise looks exactly like "Claude is idle". */
export interface ClaudeHealth {
  installed: boolean;
  version: string | null;
  connected: boolean;
  /** Some hooks present but not all — usually a hand-edited settings.json. */
  partial: boolean;
  shimInstalled: boolean;
  socketListening: boolean;
  settingsPath: string | null;
  shimPath: string | null;
  /** Epoch ms of the last hook event; null means the hooks have never fired. */
  lastEventAt: number | null;
  /** Claude's own theme setting ("dark", "custom:kelyfos-theme", …). Null when
   *  unset — Claude's default, which does not follow the terminal palette. */
  theme: string | null;
  /** The Kelyfos theme file exists in ~/.claude/themes and is current. Whether
   *  it is *selected* is `theme`. */
  themeInstalled: boolean;
}

export interface ClaudeTestResult {
  ok: boolean;
  message: string;
}

export interface FileEdits {
  path: string;
  edits: number;
}

export interface ToolUse {
  name: string;
  count: number;
}

/** Everything derivable from one Claude transcript. Wide on purpose — the
 *  display picks from it; the reader never has to be changed to add a number. */
export interface SessionStats {
  id: string;
  path: string;
  sizeBytes: number;

  startedAt: number | null;
  updatedAt: number | null;
  /** Wall clock since the first message, idle time included. */
  elapsedMs: number | null;
  /** Sum of Claude's reported turn durations: time actually spent working. */
  workingMs: number;
  turns: number;
  longestTurnMs: number;

  /** Things the user actually said. Far fewer than `userMessages`, which counts
   *  every line of role "user" — tool results included. */
  prompts: number;
  userMessages: number;
  assistantMessages: number;
  toolCalls: number;
  toolErrors: number;
  topTools: ToolUse[];
  endTurns: number;
  toolUseStops: number;
  sidechainMessages: number;
  compactions: number;

  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;

  linesAdded: number;
  linesRemoved: number;
  filesTouched: number;
  topFiles: FileEdits[];
  userModifiedAfterEdit: number;
  interruptedCommands: number;

  models: string[];
  claudeVersion: string | null;
  cwd: string | null;
  /** Often the literal "HEAD"; prefer the branch read from git itself. */
  transcriptGitBranch: string | null;
  customTitle: string | null;
  firstPrompt: string | null;
}

export interface DailyActivity {
  date: string;
  messageCount: number;
  sessionCount: number;
  toolCallCount: number;
}

export interface DailyStats {
  /** Claude recomputes this cache itself, so today may be missing. */
  lastComputedDate: string | null;
  days: DailyActivity[];
}

export interface ImportResult {
  workspace: Workspace;
}
