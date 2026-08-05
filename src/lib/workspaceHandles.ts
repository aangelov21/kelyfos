/**
 * Each mounted WorkspaceView publishes a handle here so code outside the
 * FlexLayout tree (the quick search palette) can act on a workspace's grid —
 * the model itself is local to the view and never leaves it.
 */
export interface WorkspaceHandle {
  /** Select the tab of a terminal and make its tabset the active one. */
  focusTerminal: (terminalId: string) => void;
  /** Open a local terminal in the currently focused pane. */
  openTerminal: () => void;
  /**
   * Close the terminal selected in the focused pane. Returns false when the
   * workspace has nothing to close, so the caller can tell "closed one" from
   * "there was none" — Cmd+W must do nothing rather than fall through.
   */
  closeActiveTerminal: () => boolean;
}

const handles = new Map<string, WorkspaceHandle>();

export function registerWorkspace(id: string, handle: WorkspaceHandle) {
  handles.set(id, handle);
  return () => {
    // Only drop the entry if it is still ours; a remount may have replaced it.
    if (handles.get(id) === handle) handles.delete(id);
  };
}

export function getWorkspace(id: string | null | undefined) {
  return id ? handles.get(id) : undefined;
}

/**
 * A workspace that was not open yet mounts a frame or two after being opened,
 * so an action aimed at it has to wait for its handle to appear.
 */
export function whenWorkspaceReady(
  id: string,
  run: (handle: WorkspaceHandle) => void,
  attempts = 30
) {
  const handle = handles.get(id);
  if (handle) {
    run(handle);
    return;
  }
  if (attempts <= 0) return;
  requestAnimationFrame(() => whenWorkspaceReady(id, run, attempts - 1));
}
