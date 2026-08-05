import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { FinishedMap, Heuristic } from "../lib/agentStatus";
import type { AgentSnapshot } from "../lib/types";
import { useTerminalsStore } from "./terminals";

interface AgentsState {
  /** Agent state keyed by terminal id; absent means no agent seen there. */
  byTerminal: Record<string, AgentSnapshot>;
  byTerminalHeuristic: Record<string, Heuristic>;
  /** Runs that ended while the user was looking somewhere else. */
  finished: FinishedMap;
  /** Whether Claude Code exists on this machine, checked once at boot. */
  installed: boolean;
  load: () => Promise<void>;
  subscribe: () => Promise<() => void>;
  setTitleHint: (terminalId: string, title: string) => void;
  ringBell: (terminalId: string) => void;
  clearAttention: (terminalId: string) => void;
  /** The user has now seen this terminal, so the finished badge has done its
   *  job. Separate from clearAttention: the two are set by different things and
   *  a caller that clears one may have no business clearing the other. */
  clearFinished: (terminalId: string) => void;
}

/** Claude Code sets a window title naming itself. Matching a title is guessing
 *  at another program's UI, which is why it only ever adds the marker and never
 *  drives the busy or waiting states on its own. */
const CLAUDE_TITLE = /claude/i;

/**
 * What Claude Code is doing in each terminal, kept apart from the PTY's own
 * status rather than folded into it. The two describe different things — the
 * shell's connection and the agent's work — and one field holding both would
 * mean whichever event landed last won.
 */
export const useAgentsStore = create<AgentsState>((set, get) => ({
  byTerminal: {},
  byTerminalHeuristic: {},
  finished: {},
  installed: false,

  load: async () => {
    try {
      const [list, health] = await Promise.all([
        ipc.claudeStates(),
        ipc.claudeHealth(),
      ]);
      set({
        byTerminal: Object.fromEntries(list.map((a) => [a.terminalId, a])),
        installed: health.installed,
      });
    } catch {
      // No agent state is a perfectly ordinary condition, not an error worth
      // surfacing: most users will never have Claude Code installed.
    }
  },

  subscribe: async () => {
    const unlisten = await listen<AgentSnapshot>("agent:state", (event) => {
      const snapshot = event.payload;
      const id = snapshot.terminalId;
      const previous = get().byTerminal[id]?.phase;
      const finished = { ...get().finished };

      // Only the running → idle edge counts as finishing. Running → waiting is
      // a question, which has its own louder state, and marking it here would
      // turn every permission prompt green for an instant on its way past.
      if (previous === "running" && snapshot.phase === "idle") {
        // A run the user watched end needs no badge — the same reasoning the
        // error dot uses. `document.hasFocus()` covers the app being in the
        // background entirely, where the focused terminal is nobody's view.
        const watched =
          useTerminalsStore.getState().focusedId === id && document.hasFocus();
        if (!watched) finished[id] = true;
      }
      // A new run supersedes the last result: the badge would now be pointing
      // at work that has already been overtaken.
      if (snapshot.phase === "running") delete finished[id];

      set({ byTerminal: { ...get().byTerminal, [id]: snapshot }, finished });
    });
    return unlisten;
  },

  setTitleHint: (terminalId, title) => {
    const present = CLAUDE_TITLE.test(title);
    const current = get().byTerminalHeuristic[terminalId];
    if ((current?.present ?? false) === present) return;
    set({
      byTerminalHeuristic: {
        ...get().byTerminalHeuristic,
        [terminalId]: { attention: current?.attention ?? false, present },
      },
    });
  },

  ringBell: (terminalId) => {
    const current = get().byTerminalHeuristic[terminalId];
    if (current?.attention) return;
    set({
      byTerminalHeuristic: {
        ...get().byTerminalHeuristic,
        [terminalId]: { present: current?.present ?? false, attention: true },
      },
    });
  },

  clearFinished: (terminalId) => {
    if (!get().finished[terminalId]) return;
    const { [terminalId]: _seen, ...finished } = get().finished;
    set({ finished });
  },

  clearAttention: (terminalId) => {
    const current = get().byTerminalHeuristic[terminalId];
    if (!current?.attention) return;
    set({
      byTerminalHeuristic: {
        ...get().byTerminalHeuristic,
        [terminalId]: { ...current, attention: false },
      },
    });
  },
}));
