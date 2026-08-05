import { useCallback, useEffect, useState } from "react";
// FlexLayout's stylesheet must load first — global.css overrides its theming.
import "flexlayout-react/style/dark.css";
import "./styles/global.css";

import { Sidebar } from "./components/sidebar/Sidebar";
import { TitleBar } from "./components/layout/TitleBar";
import { WorkspaceView } from "./components/layout/WorkspaceView";
import { SettingsDialog } from "./components/dialogs/SettingsDialog";
import { QuickSearch, useQuickSearchHotkey } from "./components/common/QuickSearch";
import { TooltipHost } from "./components/common/Tooltip";
import { defaultTerminalSettings } from "./lib/themes";
import { checkForUpdate } from "./lib/updater";
import { getWorkspace } from "./lib/workspaceHandles";
import { useAgentsStore } from "./stores/agents";
import { useConnectionsStore } from "./stores/connections";
import { usePresetsStore } from "./stores/presets";
import { useSettingsStore } from "./stores/settings";
import { useWorkspacesStore } from "./stores/workspaces";

const MIN_FONT_SIZE = 6;
const MAX_FONT_SIZE = 40;

/**
 * Ctrl/Cmd +/- resizes the terminal font (Ctrl/Cmd+0 resets it), for every
 * terminal at once since the size lives in settings. Listening in the capture
 * phase keeps the combo away from both xterm — which would otherwise send it
 * to the shell — and the webview's own page zoom.
 */
function useTerminalZoom() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      // "=" and "_" are the unshifted/shifted twins of the +/- keys.
      const step =
        e.key === "+" || e.key === "=" || e.code === "NumpadAdd"
          ? 1
          : e.key === "-" || e.key === "_" || e.code === "NumpadSubtract"
            ? -1
            : 0;
      const reset = e.key === "0" || e.code === "Numpad0";
      if (step === 0 && !reset) return;

      e.preventDefault();
      e.stopPropagation();

      const { terminal, setTerminal } = useSettingsStore.getState();
      const next = reset
        ? defaultTerminalSettings.fontSize
        : Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, terminal.fontSize + step));
      if (next !== terminal.fontSize) setTerminal({ fontSize: next });
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}

/**
 * Kill the webview's own context menu app-wide.
 *
 * This is a desktop app, not a page: "Reload", "Back", "Inspect Element" and
 * "Open Link in New Window" are all either meaningless here or a way out of the
 * app's own UI. Nothing is lost — no element relies on the native menu, and
 * right-click in a terminal is not bound to anything.
 *
 * The app's own menus are unaffected. They are React onContextMenu handlers, and
 * this listener only calls preventDefault() — which suppresses the native menu
 * whoever calls it — so both still run and only the native one is dropped.
 */
function useNoNativeContextMenu() {
  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);
}

/**
 * Ctrl/Cmd+W closes the terminal selected in the active workspace. The native
 * "Close Window" item is stripped from the menu (see menu.rs) so the combo
 * reaches the webview at all; here it is swallowed unconditionally, because a
 * workspace with nothing to close must do nothing rather than let xterm send
 * ^W to the shell.
 */
function useCloseTerminalHotkey() {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
      if (e.key !== "w" && e.key !== "W") return;

      e.preventDefault();
      e.stopPropagation();

      const { activeId } = useWorkspacesStore.getState();
      getWorkspace(activeId)?.closeActiveTerminal();
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}

function App() {
  const [ready, setReady] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<"terminal" | "advanced">("terminal");
  const [quickSearchOpen, setQuickSearchOpen] = useState(false);

  const workspaces = useWorkspacesStore((s) => s.workspaces);
  const openIds = useWorkspacesStore((s) => s.openIds);
  const activeId = useWorkspacesStore((s) => s.activeId);

  useTerminalZoom();
  useCloseTerminalHotkey();
  useNoNativeContextMenu();
  useQuickSearchHotkey(useCallback(() => setQuickSearchOpen(true), []));

  // Agent state, if anything is reporting any. Independent of boot: a machine
  // without Claude Code simply never receives an event, and nothing here can
  // hold up the terminals.
  useEffect(() => {
    useAgentsStore.getState().load();
    const pending = useAgentsStore.getState().subscribe();
    return () => {
      pending.then((unlisten) => unlisten());
    };
  }, []);

  // Boot: settings → data → restore previous session.
  useEffect(() => {
    (async () => {
      try {
        await useSettingsStore.getState().load();
        await Promise.all([
          useConnectionsStore.getState().load(),
          usePresetsStore.getState().load(),
          useWorkspacesStore.getState().load(),
        ]);
        await useWorkspacesStore.getState().restoreSession();
      } catch (e) {
        console.error("boot failed:", e);
      }
      setReady(true);

      // Opt-in, and only ever surfaced when there is something to install — a
      // failed check at boot (offline) stays silent rather than nagging.
      if (useSettingsStore.getState().ui.checkUpdatesOnStartup) {
        const result = await checkForUpdate();
        if (result.kind === "available") {
          setSettingsSection("advanced");
          setSettingsOpen(true);
        }
      }
    })();
  }, []);

  if (!ready) return null;

  return (
    <div className="app">
      <TitleBar
        onOpenSettings={() => {
          setSettingsSection("terminal");
          setSettingsOpen(true);
        }}
      />
      <div className="app-body">
        <Sidebar onOpenQuickSearch={() => setQuickSearchOpen(true)} />
        <div className="main">
          <div className="workspace-area">
            {openIds.length === 0 && (
              <div className="empty-state">
                <h2>No workspace open</h2>
                <span>Select or create a workspace in the sidebar.</span>
              </div>
            )}
            {openIds.map((id) => {
              const w = workspaces.find((x) => x.id === id);
              if (!w) return null;
              return <WorkspaceView key={id} workspace={w} visible={id === activeId} />;
            })}
          </div>
        </div>
      </div>
      {quickSearchOpen && <QuickSearch onClose={() => setQuickSearchOpen(false)} />}
      {settingsOpen && (
        <SettingsDialog
          initialSection={settingsSection}
          onClose={() => setSettingsOpen(false)}
        />
      )}
      <TooltipHost />
    </div>
  );
}

export default App;
