import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { Workspace } from "../lib/types";

interface WorkspacesState {
  workspaces: Workspace[];
  openIds: string[];
  activeId: string | null;
  search: string;
  load: () => Promise<void>;
  restoreSession: () => Promise<void>;
  open: (id: string) => void;
  close: (id: string) => void;
  setActive: (id: string) => void;
  save: (w: Workspace) => Promise<Workspace>;
  remove: (id: string) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
  saveLayout: (id: string, layout: unknown) => void;
  setSearch: (s: string) => void;
}

function persistSession(_openIds: string[], activeId: string | null) {
  ipc.stateSet("active_workspace", activeId);
}

const layoutSaveTimers = new Map<string, ReturnType<typeof setTimeout>>();

export const useWorkspacesStore = create<WorkspacesState>((set, get) => ({
  workspaces: [],
  openIds: [],
  activeId: null,
  search: "",

  load: async () => {
    set({ workspaces: await ipc.workspacesList() });
  },

  restoreSession: async () => {
    // Only the active workspace is restored — the sidebar is the switcher.
    const activeId = await ipc.stateGet<string>("active_workspace");
    const exists = activeId && get().workspaces.some((w) => w.id === activeId);
    set({
      openIds: exists ? [activeId] : [],
      activeId: exists ? activeId : null,
    });
  },

  open: (id) => {
    const { openIds } = get();
    const next = openIds.includes(id) ? openIds : [...openIds, id];
    set({ openIds: next, activeId: id });
    persistSession(next, id);
  },

  close: (id) => {
    const { openIds, activeId } = get();
    const next = openIds.filter((x) => x !== id);
    const nextActive = activeId === id ? next[next.length - 1] ?? null : activeId;
    set({ openIds: next, activeId: nextActive });
    persistSession(next, nextActive);
  },

  setActive: (id) => {
    set({ activeId: id });
    persistSession(get().openIds, id);
  },

  save: async (w) => {
    const saved = await ipc.workspacesSave(w);
    await get().load();
    return saved;
  },

  remove: async (id) => {
    await ipc.workspacesDelete(id);
    get().close(id);
    await get().load();
  },

  duplicate: async (id) => {
    await ipc.workspaceDuplicate(id);
    await get().load();
  },

  saveLayout: (id, layout) => {
    // Update local copy immediately; write-through debounced.
    set({
      workspaces: get().workspaces.map((w) => (w.id === id ? { ...w, layout } : w)),
    });
    const prev = layoutSaveTimers.get(id);
    if (prev) clearTimeout(prev);
    layoutSaveTimers.set(
      id,
      setTimeout(() => {
        const w = get().workspaces.find((x) => x.id === id);
        if (w) ipc.workspacesSave(w);
      }, 500)
    );
  },

  setSearch: (search) => set({ search }),
}));
