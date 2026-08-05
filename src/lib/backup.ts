import { ipc } from "./ipc";
import type { TagColorMap } from "./tags";
import type { AppTheme } from "./themes";
import type { Connection, Preset, Variable } from "./types";

const BACKUP_TYPE = "kelyfos-backup";
const BACKUP_VERSION = 1;

interface Backup {
  type: string;
  version: number;
  exportedAt: string;
  /** Raw settings map: ui, terminal, customThemes, tagColors. */
  settings: Record<string, unknown>;
  connections: Connection[];
  /** Presets with their own variables inlined, since those travel together. */
  presets: { preset: Preset; variables: Variable[] }[];
  globalVariables: Variable[];
  /** Each entry is a `{ version, workspace }` object from workspace_export. */
  workspaces: unknown[];
}

/** Bundle every session setting into one portable JSON string. */
export async function exportAll(): Promise<string> {
  const [settings, connections, presets, globalVariables, workspaces] = await Promise.all([
    ipc.settingsGetAll(),
    ipc.connectionsList(),
    ipc.presetsList(),
    ipc.variablesList("global", null),
    ipc.workspacesList(),
  ]);

  const workspaceExports = await Promise.all(
    workspaces.map((w) => ipc.workspaceExport(w.id).then((s) => JSON.parse(s)))
  );

  const presetExports = await Promise.all(
    presets.map(async (preset) => ({
      preset,
      variables: await ipc.variablesList("preset", preset.id),
    }))
  );

  const bundle: Backup = {
    type: BACKUP_TYPE,
    version: BACKUP_VERSION,
    exportedAt: new Date().toISOString(),
    settings,
    connections,
    presets: presetExports,
    globalVariables,
    workspaces: workspaceExports,
  };
  return JSON.stringify(bundle, null, 2);
}

/**
 * Restore a backup additively: settings and custom themes are merged, while
 * connections, global variables and workspaces are recreated as new entries
 * (re-importing your own backup therefore duplicates them rather than wiping
 * what you have).
 */
export async function importAll(jsonText: string): Promise<void> {
  const bundle = JSON.parse(jsonText) as Partial<Backup>;
  if (bundle.type !== BACKUP_TYPE) {
    throw new Error("Not a Kelyfos backup file.");
  }
  if ((bundle.version ?? 0) > BACKUP_VERSION) {
    throw new Error(`Unsupported backup version: ${bundle.version}`);
  }

  const settings = bundle.settings ?? {};
  if (settings.ui) await ipc.settingsSet("ui", settings.ui);
  if (settings.terminal) await ipc.settingsSet("terminal", settings.terminal);

  // Merge custom themes by id so a restore never drops themes already present.
  if (Array.isArray(settings.customThemes)) {
    const current = ((await ipc.settingsGetAll())["customThemes"] as AppTheme[]) ?? [];
    const byId = new Map(current.map((t) => [t.id, t]));
    for (const t of settings.customThemes as AppTheme[]) byId.set(t.id, t);
    await ipc.settingsSet("customThemes", [...byId.values()]);
  }

  // Tag colours come across with the tags that ride on the imported records.
  // A colour already set locally wins: a tag is identified by its name, so the
  // same name must not end up painted two ways, and the colour this machine is
  // already reading its sidebar by is the one to keep.
  if (settings.tagColors && typeof settings.tagColors === "object") {
    const current = ((await ipc.settingsGetAll())["tagColors"] as TagColorMap) ?? {};
    await ipc.settingsSet("tagColors", {
      ...(settings.tagColors as TagColorMap),
      ...current,
    });
  }

  // Imported connections get fresh ids, so remember the mapping — a preset's
  // connectionId still points at the id the backup was written with.
  const connectionIds = new Map<string, string>();
  for (const c of bundle.connections ?? []) {
    const saved = await ipc.connectionsSave({ ...c, id: "" });
    connectionIds.set(c.id, saved.id);
  }

  for (const entry of bundle.presets ?? []) {
    const saved = await ipc.presetsSave({
      ...entry.preset,
      id: "",
      createdAt: "",
      connectionId: entry.preset.connectionId
        ? connectionIds.get(entry.preset.connectionId) ?? null
        : null,
    });
    for (const v of entry.variables) {
      await ipc.variablesSet({ ...v, id: "", scope: "preset", ownerId: saved.id });
    }
  }

  for (const v of bundle.globalVariables ?? []) {
    await ipc.variablesSet({ ...v, id: "", scope: "global", ownerId: null });
  }

  for (const w of bundle.workspaces ?? []) {
    await ipc.workspaceImport(JSON.stringify(w));
  }
}
