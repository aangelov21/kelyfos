import { ipc } from "./ipc";
import type { Preset } from "./types";
import { useTerminalsStore } from "../stores/terminals";
import { useWorkspacesStore } from "../stores/workspaces";

/**
 * Creates the terminal a preset describes. Everything is copied, not linked:
 * the connection, shell and startup tasks land on the terminal def, and the
 * preset's variables are duplicated into the terminal's own scope. Editing
 * either side afterwards leaves the other alone.
 *
 * `id` may be supplied so a caller that has already inserted a tab (a drop on
 * the layout) can keep the tab and the def in step — terminals_save honours it.
 */
export async function terminalFromPreset(
  preset: Preset,
  workspaceId: string,
  id = ""
) {
  // Deliberately not the store's save(): that reloads the workspace, which
  // mounts the pane and opens the PTY, and pty_open resolves ${VARS} once at
  // that moment. Copying first and only then reloading keeps the def out of
  // the store until its variables exist.
  const terminal = await ipc.terminalsSave({
    id,
    workspaceId,
    connectionId: preset.connectionId,
    title: preset.name,
    startupTasks: preset.startupTasks,
    sortOrder: 0,
    shell: preset.shell,
  });
  await ipc.presetCopyVariables(preset.id, terminal.id).catch(() => {});
  await useTerminalsStore.getState().load(workspaceId);
  return terminal;
}

/**
 * Open a preset as a terminal in the focused pane of the active workspace,
 * falling back to an auto-created "Terminals" workspace when none is open.
 * Same shape as openConnectionTerminal.
 */
export async function openPresetTerminal(preset: Preset) {
  const wsState = useWorkspacesStore.getState();
  let ws = wsState.workspaces.find((w) => w.id === wsState.activeId);
  if (!ws) {
    ws =
      wsState.workspaces.find((w) => w.name === "Terminals") ??
      (await wsState.save({
        id: "",
        name: "Terminals",
        description: "Standalone terminals",
        tags: [],
        icon: "terminal",
        layout: null,
        createdAt: "",
        updatedAt: "",
      }));
  }
  await terminalFromPreset(preset, ws.id);
  wsState.open(ws.id);
}
