import { create } from "zustand";
import { ipc } from "../lib/ipc";
import {
  aggregateStatus,
  effectiveStatus,
  type ActivityMap,
  type Signals,
} from "../lib/agentStatus";
import type { TerminalDef, TerminalStatus } from "../lib/types";

export interface StatusEntry {
  status: TerminalStatus;
  message?: string;
}

interface TerminalsState {
  /** terminal defs keyed by workspace id */
  byWorkspace: Record<string, TerminalDef[]>;
  statuses: Record<string, StatusEntry>;
  /** Bumping an epoch remounts the xterm view, which reconnects the session. */
  epochs: Record<string, number>;
  /**
   * Directory a terminal's shell starts in, keyed by terminal id. Set when a
   * new terminal inherits the active one's cwd; not persisted, so after a
   * restart terminals open in the home directory again.
   */
  cwds: Record<string, string>;
  setCwd: (terminalId: string, cwd: string) => void;
  load: (workspaceId: string) => Promise<void>;
  save: (t: TerminalDef) => Promise<TerminalDef>;
  remove: (t: TerminalDef) => Promise<void>;
  /** Terminals that printed something while nobody was watching them. */
  activity: ActivityMap;
  noteActivity: (terminalId: string) => void;
  clearActivity: (terminalId: string) => void;
  /** Terminal the user is currently typing in, if any. */
  focusedId: string | null;
  setFocused: (terminalId: string | null) => void;
  setStatus: (terminalId: string, entry: StatusEntry) => void;
  /** Drop a latched error once the user has seen the tab. */
  clearError: (terminalId: string) => void;
  reconnect: (terminalId: string) => void;
  get: (terminalId: string) => TerminalDef | undefined;
}

export const useTerminalsStore = create<TerminalsState>((set, get) => ({
  byWorkspace: {},
  statuses: {},
  epochs: {},
  cwds: {},
  activity: {},
  focusedId: null,

  // Called for every chunk of output that arrives unwatched, so the already-set
  // case has to cost nothing: writing the same value would re-render every
  // subscriber on each read of a `tail -f`.
  noteActivity: (terminalId) => {
    if (get().activity[terminalId]) return;
    set({ activity: { ...get().activity, [terminalId]: true } });
  },

  clearActivity: (terminalId) => {
    if (!get().activity[terminalId]) return;
    const { [terminalId]: _seen, ...activity } = get().activity;
    set({ activity });
  },

  setCwd: (terminalId, cwd) =>
    set({ cwds: { ...get().cwds, [terminalId]: cwd } }),

  load: async (workspaceId) => {
    const list = await ipc.terminalsList(workspaceId);
    set({ byWorkspace: { ...get().byWorkspace, [workspaceId]: list } });
  },

  save: async (t) => {
    const saved = await ipc.terminalsSave(t);
    await get().load(t.workspaceId);
    return saved;
  },

  remove: async (t) => {
    ipc.ptyClose(t.id).catch(() => {});
    const { [t.id]: _dropped, ...cwds } = get().cwds;
    set({ cwds });
    await ipc.terminalsDelete(t.id);
    await get().load(t.workspaceId);
  },

  setFocused: (terminalId) => set({ focusedId: terminalId }),

  setStatus: (terminalId, entry) => {
    // A command that failed in the terminal the user is typing in was watched
    // as it happened; the dot exists to report failures that happened while
    // they were somewhere else, and blinking at someone who just read the error
    // is how an indicator earns being ignored.
    const seen =
      entry.status === "error" &&
      terminalId === get().focusedId &&
      document.hasFocus();
    const next: StatusEntry = seen ? { status: "connected" } : entry;
    set({ statuses: { ...get().statuses, [terminalId]: next } });
  },

  clearError: (terminalId) => {
    const current = get().statuses[terminalId];
    // Only an error clears this way. A terminal that is busy or disconnected is
    // reporting something still true, which looking at it does not change.
    if (current?.status !== "error") return;
    set({
      statuses: { ...get().statuses, [terminalId]: { status: "connected" } },
    });
  },

  reconnect: (terminalId) =>
    set({
      epochs: { ...get().epochs, [terminalId]: (get().epochs[terminalId] ?? 0) + 1 },
    }),

  get: (terminalId) => {
    for (const list of Object.values(get().byWorkspace)) {
      const found = list.find((t) => t.id === terminalId);
      if (found) return found;
    }
    return undefined;
  },
}));

/** Aggregate a workspace's terminal statuses into one tab badge status. */
export function workspaceStatus(
  terminals: TerminalDef[] | undefined,
  statuses: Record<string, StatusEntry>,
  signals: Signals = {}
): TerminalStatus {
  if (!terminals) return "idle";
  return aggregateStatus(
    terminals.map((t) =>
      effectiveStatus(t.id, statuses[t.id]?.status ?? "idle", signals)
    )
  );
}
