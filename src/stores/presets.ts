import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { Preset } from "../lib/types";

export function emptyPreset(): Preset {
  return {
    id: "",
    name: "",
    description: "",
    connectionId: null,
    startupTasks: [],
    shell: "",
    color: "#a78bfa",
    tags: [],
    sortOrder: 0,
    createdAt: "",
    updatedAt: "",
  };
}

interface PresetsState {
  presets: Preset[];
  load: () => Promise<void>;
  save: (p: Preset) => Promise<Preset>;
  remove: (id: string) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
}

export const usePresetsStore = create<PresetsState>((set, get) => ({
  presets: [],

  load: async () => {
    set({ presets: await ipc.presetsList() });
  },

  save: async (p) => {
    const saved = await ipc.presetsSave(p);
    await get().load();
    return saved;
  },

  remove: async (id) => {
    await ipc.presetsDelete(id);
    await get().load();
  },

  // The copy gets its own id, so its variables (scope "preset", owner the id)
  // start empty rather than being shared with the original.
  duplicate: async (id) => {
    const src = get().presets.find((p) => p.id === id);
    if (!src) return;
    await ipc.presetsSave({ ...src, id: "", name: `${src.name} (copy)`, createdAt: "" });
    await get().load();
  },
}));
