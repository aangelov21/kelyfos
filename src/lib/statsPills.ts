/**
 * The catalogue of pills the terminal status bar can show.
 *
 * One list, used both to render the bar and to build the picker in Settings —
 * so a pill can never exist in one place and be missing from the other, and the
 * display order is defined once.
 */
export interface StatPillDef {
  key: string;
  /** Shown on the pill itself. */
  label: string;
  /** Shown in the Settings picker, where there is room to say what it means. */
  description: string;
}

export const STAT_PILLS: StatPillDef[] = [
  {
    key: "branch",
    label: "branch",
    description: "Current git branch of the terminal's directory",
  },
  {
    key: "session",
    label: "session",
    description: "Time since the Claude session started, idle time included",
  },
  {
    key: "working",
    label: "working",
    description: "Time Claude actually spent working, summed across turns",
  },
  {
    key: "prompts",
    label: "prompts",
    description: "Things you asked for — not counting tool results",
  },
  { key: "turns", label: "turns", description: "Completed turns" },
  {
    key: "lines",
    label: "lines",
    description: "Lines added and removed by Claude's edits",
  },
  { key: "files", label: "files", description: "Files touched this session" },
  { key: "tools", label: "tools", description: "Tool calls made" },
  {
    key: "toolErrors",
    label: "tool errors",
    description: "Tool calls that came back as errors",
  },
  { key: "output", label: "out", description: "Output tokens" },
  {
    key: "input",
    label: "in",
    description: "Fresh input tokens, excluding cache reads",
  },
  {
    key: "cacheRead",
    label: "cache read",
    description: "Tokens served from the prompt cache",
  },
  {
    key: "cacheWrite",
    label: "cache write",
    description: "Tokens written to the prompt cache",
  },
  {
    key: "compactions",
    label: "compactions",
    description: "Times the conversation was compacted — context pressure",
  },
  {
    key: "rework",
    label: "rework",
    description: "Files you changed after Claude had edited them",
  },
  {
    key: "interrupted",
    label: "interrupted",
    description: "Tool calls interrupted before finishing",
  },
  {
    key: "sidechain",
    label: "subagent",
    description: "Messages from subagents inside this session",
  },
  {
    key: "size",
    label: "transcript",
    description: "Size of the session's transcript file",
  },
  { key: "model", label: "model", description: "Model, or models if it changed" },
  {
    key: "version",
    label: "claude",
    description: "Claude Code version that wrote the session",
  },
  {
    key: "id",
    label: "session id",
    description: "Session id — click the pill to copy it for --resume",
  },
];

/** Everything on, matching what the bar showed before it was configurable. */
export const DEFAULT_STAT_PILLS: string[] = STAT_PILLS.map((p) => p.key);
