import type { Connection } from "./types";
import { useTerminalsStore } from "../stores/terminals";
import { useWorkspacesStore } from "../stores/workspaces";

/**
 * Open a saved connection as a terminal in the currently active workspace, in
 * whichever pane has the focus (the layout adds new tabs to the active tabset).
 * With no workspace open it falls back to an auto-created "Terminals" one.
 */
export async function openConnectionTerminal(c: Connection) {
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
  await useTerminalsStore.getState().save({
    id: "",
    workspaceId: ws.id,
    connectionId: c.id,
    title: c.name,
    startupTasks: [],
    sortOrder: 0,
    shell: "",
  });
  wsState.open(ws.id);
}
