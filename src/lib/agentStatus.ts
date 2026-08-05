import type { AgentSnapshot, TerminalStatus } from "./types";

/**
 * What the terminal itself gave away, for the cases the backend cannot see:
 * an SSH session, where Claude runs on the far end and its hooks cannot reach
 * this machine at all, and a user who never connected. Both signals come from
 * another program's output, so they stay separate from the authoritative state
 * and never override it.
 */
export interface Heuristic {
  /** The window title named Claude. */
  present: boolean;
  /** The terminal rang the bell and has not been looked at since. */
  attention: boolean;
}

export type AgentMap = Record<string, AgentSnapshot>;
export type HeuristicMap = Record<string, Heuristic>;
/**
 * Terminals where Claude finished a run the user has not looked at yet. Set on
 * the running → not-running edge, cleared the moment the terminal is focused,
 * so it only ever describes work that ended somewhere the user was not looking.
 */
export type FinishedMap = Record<string, boolean>;
/**
 * Terminals that printed something while nobody was looking at them. Same shape
 * and same life cycle as [[FinishedMap]]: set by the thing that noticed,
 * cleared by the user arriving.
 */
export type ActivityMap = Record<string, boolean>;

/**
 * Everything that can colour a dot besides the PTY's own status. Passed as one
 * object rather than four positional maps — the list has grown twice, and a
 * call site silently passing `finished` where `activity` was expected is the
 * kind of bug that types would not catch.
 */
export interface Signals {
  agents?: AgentMap;
  heuristics?: HeuristicMap;
  finished?: FinishedMap;
  activity?: ActivityMap;
}

/**
 * Pure status resolution, kept apart from the store so it can be reasoned about
 * (and exercised) on its own. Every bug this has had was a precedence mistake
 * rather than a wiring one.
 */
export function effectiveStatus(
  terminalId: string,
  ptyStatus: TerminalStatus,
  signals: Signals = {}
): TerminalStatus {
  const { agents = {}, heuristics = {}, finished = {}, activity = {} } = signals;
  const agent = agents[terminalId];
  const heuristic = heuristics[terminalId];

  if (agent?.present) {
    if (agent.phase === "waiting") return "ai_waiting";
    if (agent.phase === "running") return "ai_running";
  }

  // Outranked by both live states above, so a run that ends by asking a question
  // still reads as a question. Below them and above everything else: it is the
  // one state here that exists purely to be noticed from another terminal.
  // Survives the agent going away, because a session that finished and exited is
  // still a result the user has not seen.
  if (finished[terminalId]) return "ai_done";

  if (agent?.present) {
    // Claude is open but doing nothing. The shell disagrees — `claude` is one
    // long-running command, so the prompt marks report a command in progress
    // from launch until exit — but that is the shell describing its own view,
    // not work in progress, and letting it through is what makes an idle agent
    // blink forever. The agent owns this terminal's badge now.
    return ptyStatus === "busy" ? "connected" : ptyStatus;
  }

  // Only consulted where nothing authoritative is reporting: a bell means some
  // program asked for the user, which is the same thing the waiting state says.
  if (heuristic?.attention) return "ai_waiting";

  // A failure outranks new output: both say "look here", and only one of them
  // says why. Below that, unseen output outranks "busy" on purpose — a `tail
  // -f` is busy for as long as it runs, so reporting busy would mean the dot
  // never changed no matter how much arrived.
  if (ptyStatus === "error") return "error";
  if (activity[terminalId]) return "activity";

  // Same suppression for the title-only case (an SSH session, say): we know a
  // TUI is holding the foreground, which is precisely why "busy" is misleading.
  if (heuristic?.present) {
    return ptyStatus === "busy" ? "connected" : ptyStatus;
  }
  return ptyStatus;
}

/** Whether to mark this terminal as running Claude. */
export function hasClaude(
  terminalId: string,
  agents: AgentMap,
  heuristics: HeuristicMap = {}
): boolean {
  return (
    agents[terminalId]?.present === true ||
    heuristics[terminalId]?.present === true
  );
}

/** Tooltip text for a terminal running Claude, or null when none is. */
export function agentTooltip(
  agent: AgentSnapshot | undefined,
  heuristic?: Heuristic,
  finished = false
): string | null {
  if (agent?.present) {
    if (agent.phase === "waiting") return "Claude is waiting for you";
    if (agent.phase === "running") {
      return agent.tool ? `Claude is running ${agent.tool}` : "Claude is working";
    }
  }
  // Same precedence as the badge, so the words never disagree with the colour.
  if (finished) return "Claude finished while you were elsewhere";
  if (agent?.present) return "Claude is open";
  if (heuristic?.attention) return "This terminal asked for your attention";
  return heuristic?.present ? "Claude is open" : null;
}

/**
 * Aggregate several terminals into one badge. An agent blocked on the user
 * comes first: it is the only state here that stays stuck until somebody acts
 * on it, and it is cheap to miss inside a collapsed workspace.
 */
export function aggregateStatus(list: TerminalStatus[]): TerminalStatus {
  if (list.length === 0) return "idle";
  if (list.includes("ai_waiting")) return "ai_waiting";
  // Live work outranks a finished run: while anything is still going the
  // workspace is not done, and the green would claim otherwise. The finished
  // terminal keeps its own green dot either way — this only decides what the
  // one dot standing for the whole workspace says.
  if (list.includes("ai_running")) return "ai_running";
  if (list.includes("ai_done")) return "ai_done";
  // Work in progress outranks a failure: a workspace with something still
  // running is the one worth looking at, and the error stays visible on the
  // terminal's own tab either way.
  if (list.includes("busy")) return "busy";
  if (list.includes("error")) return "error";
  // Last of the "look at me" states, because it is the one that says least: a
  // terminal printed something. Its own tab carries the dot regardless.
  if (list.includes("activity")) return "activity";
  if (list.includes("connecting")) return "connecting";
  if (list.includes("connected")) return "connected";
  if (list.includes("disconnected")) return "disconnected";
  return "idle";
}
