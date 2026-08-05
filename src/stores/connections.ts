import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { Connection } from "../lib/types";
import { usePresetsStore } from "./presets";

export function emptyConnection(): Connection {
  return {
    id: "",
    name: "",
    host: "",
    port: 22,
    username: "",
    description: "",
    icon: "server",
    color: "#4f8cff",
    tags: [],
    folder: "",
    authMethod: "password",
    keyPath: null,
    createdAt: "",
    updatedAt: "",
  };
}

interface ConnectionsState {
  connections: Connection[];
  search: string;
  load: () => Promise<void>;
  save: (c: Connection) => Promise<Connection>;
  remove: (id: string) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
  setSearch: (s: string) => void;
}

export const useConnectionsStore = create<ConnectionsState>((set, get) => ({
  connections: [],
  search: "",

  load: async () => {
    set({ connections: await ipc.connectionsList() });
  },

  save: async (c) => {
    const saved = await ipc.connectionsSave(c);
    await get().load();
    return saved;
  },

  // Deleting a connection also deletes the presets built on it (they would
  // have nothing left to open), so that list has to be refreshed as well.
  remove: async (id) => {
    await ipc.connectionsDelete(id);
    await get().load();
    await usePresetsStore.getState().load();
  },

  duplicate: async (id) => {
    const src = get().connections.find((c) => c.id === id);
    if (!src) return;
    await ipc.connectionsSave({
      ...src,
      id: "",
      name: `${src.name} (copy)`,
    });
    await get().load();
  },

  setSearch: (search) => set({ search }),
}));
