import { useEffect, useMemo, useRef, useState } from "react";
import {
  Action,
  Actions,
  DockLocation,
  Layout,
  Model,
  TabNode,
  TabSetNode,
} from "flexlayout-react";
import type {
  DragRectRenderCallback,
  NodeMouseEvent,
  ShowOverflowMenuCallback,
} from "flexlayout-react";
import {
  getDraggedConnection,
  isConnectionDrag,
} from "../../lib/dragConnection";
import { getDraggedPreset, isPresetDrag } from "../../lib/dragPreset";
import { ipc } from "../../lib/ipc";
import { terminalFromPreset } from "../../lib/openPreset";
import { registerWorkspace } from "../../lib/workspaceHandles";
import type { Connection, TerminalDef, Workspace } from "../../lib/types";
import { useConnectionsStore } from "../../stores/connections";
import { agentTooltip, effectiveStatus, hasClaude } from "../../lib/agentStatus";
import { useAgentsStore } from "../../stores/agents";
import { useTerminalsStore } from "../../stores/terminals";
import { ClaudeMark } from "../common/ClaudeMark";
import { ClaudeSessionsDialog } from "../dialogs/ClaudeSessionsDialog";
import { useWorkspacesStore } from "../../stores/workspaces";
import { TerminalPane } from "./TerminalPane";
import { TabTitle } from "./TabTitle";
import { TerminalCountIcon } from "./TerminalCountIcon";
import { ContextMenu, MenuItem } from "../common/ContextMenu";
import { TerminalDialog } from "../dialogs/TerminalDialog";

/** Keeps a pane from being dragged shut against the edge of the workspace. */
const MIN_PANE_SIZE = 20;

const GLOBAL_ATTRS = {
  tabEnableRename: false,
  tabSetEnableMaximize: false,
  tabSetMinWidth: MIN_PANE_SIZE,
  tabSetMinHeight: MIN_PANE_SIZE,
};

const DEFAULT_LAYOUT = {
  global: GLOBAL_ATTRS,
  borders: [],
  layout: {
    type: "row",
    children: [{ type: "tabset", weight: 100, children: [] }],
  },
};

interface Props {
  workspace: Workspace;
  visible: boolean;
}

export function WorkspaceView({ workspace, visible }: Props) {
  const terminals = useTerminalsStore((s) => s.byWorkspace[workspace.id]);
  const statuses = useTerminalsStore((s) => s.statuses);
  const loadTerminals = useTerminalsStore((s) => s.load);
  const removeTerminal = useTerminalsStore((s) => s.remove);
  const saveTerminal = useTerminalsStore((s) => s.save);
  const reconnect = useTerminalsStore((s) => s.reconnect);
  const connections = useConnectionsStore((s) => s.connections);
  const saveLayout = useWorkspacesStore((s) => s.saveLayout);

  const setCwd = useTerminalsStore((s) => s.setCwd);
  const clearError = useTerminalsStore((s) => s.clearError);
  const activity = useTerminalsStore((s) => s.activity);
  const clearActivity = useTerminalsStore((s) => s.clearActivity);
  const agents = useAgentsStore((s) => s.byTerminal);
  const heuristics = useAgentsStore((s) => s.byTerminalHeuristic);
  const finished = useAgentsStore((s) => s.finished);
  const claudeInstalled = useAgentsStore((s) => s.installed);
  const clearAttention = useAgentsStore((s) => s.clearAttention);
  const clearFinished = useAgentsStore((s) => s.clearFinished);
  const [resuming, setResuming] = useState<TerminalDef | null>(null);

  const [editing, setEditing] = useState<TerminalDef | null>(null);
  const [dropTarget, setDropTarget] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(
    null
  );
  /** Tabset the next terminal should open in, set by that tabset's + button. */
  const pendingTabsetRef = useRef<string | null>(null);

  const model = useMemo(() => {
    const json =
      workspace.layout && typeof workspace.layout === "object" && (workspace.layout as any).layout
        ? (workspace.layout as any)
        : DEFAULT_LAYOUT;
    let m: Model;
    try {
      m = Model.fromJson(json);
    } catch {
      m = Model.fromJson(DEFAULT_LAYOUT);
    }
    // Also applies to layouts saved before these globals changed.
    m.doAction(Actions.updateModelAttributes(GLOBAL_ATTRS));
    return m;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace.id]);

  useEffect(() => {
    loadTerminals(workspace.id);
  }, [workspace.id, loadTerminals]);

  const tabName = (t: TerminalDef) =>
    t.title || connections.find((c) => c.id === t.connectionId)?.name || "Terminal";

  /**
   * New terminals open as a full tab in the active tabset — splitting is left
   * to the user via drag & drop.
   * Note: ids declared in the layout JSON end up detached after
   * Model.fromJson normalization, so always target a live tabset node.
   */
  const addTabForTerminal = (t: TerminalDef, select: boolean) => {
    const json = { type: "tab", id: t.id, name: tabName(t), component: "terminal" };
    // A + button names its own tabset, which is not necessarily the focused
    // one; anything else lands in the active tabset as before.
    const requested = pendingTabsetRef.current
      ? model.getNodeById(pendingTabsetRef.current)
      : undefined;
    pendingTabsetRef.current = null;
    const target =
      (requested instanceof TabSetNode ? requested : undefined) ??
      model.getActiveTabset() ??
      model.getFirstTabSet();
    if (!target) return;
    model.doAction(Actions.addNode(json, target.getId(), DockLocation.CENTER, -1, select));
  };

  // Keep layout in sync with terminal defs (prune deleted, add missing).
  useEffect(() => {
    if (!terminals) return;
    const ids = new Set(terminals.map((t) => t.id));
    const stale: string[] = [];
    model.visitNodes((node) => {
      if (node.getType() === "tab" && !ids.has(node.getId())) stale.push(node.getId());
    });
    stale.forEach((id) => model.doAction(Actions.deleteTab(id)));
    for (const t of terminals) {
      if (!model.getNodeById(t.id)) addTabForTerminal(t, true);
    }
    if (stale.length > 0) {
      saveLayout(workspace.id, model.toJson());
      remeasureTabStrips();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminals, model]);

  // Resolve ${VARS} in titles and apply them to the layout tabs.
  useEffect(() => {
    if (!terminals) return;
    for (const t of terminals) {
      const raw = tabName(t);
      const apply = (name: string) => {
        const node = model.getNodeById(t.id) as TabNode | undefined;
        if (node && node.getName() !== name) {
          model.doAction(Actions.renameTab(t.id, name));
        }
      };
      if (raw.includes("${")) {
        ipc.variablesResolve(workspace.id, t.id).then((vars) => {
          apply(raw.replace(/\$\{([^}]+)\}/g, (m, k) => vars[k] ?? m));
        });
      } else {
        apply(raw);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminals, connections, model]);

  if (import.meta.env.DEV) (window as any).__model = model;

  const factory = (node: TabNode) => {
    const terminal = terminals?.find((t) => t.id === node.getId());
    if (!terminal) return <div className="empty-state">Terminal not found</div>;
    return <TerminalPane terminal={terminal} />;
  };

  /**
   * A connection dropped on the layout becomes a terminal in this workspace.
   * The tab id is generated here so the node FlexLayout inserts at the drop
   * location already matches the terminal def that is saved right after
   * (terminals_save honours a supplied id).
   */
  const createTerminalForConnection = (c: Connection, id: string) =>
    saveTerminal({
      id,
      workspaceId: workspace.id,
      connectionId: c.id,
      title: c.name,
      startupTasks: [],
      sortOrder: 0,
      shell: "",
    });

  /**
   * The + button opens a plain local terminal straight away — no dialog. It
   * starts in the directory `fromTerminalId` is currently in; when that can't
   * be read (or there is no terminal to inherit from) the shell opens in ~.
   */
  const openLocalTerminal = async (fromTerminalId?: string, tabsetId?: string) => {
    pendingTabsetRef.current = tabsetId ?? null;
    const id = crypto.randomUUID();
    if (fromTerminalId) {
      const cwd = await ipc.ptyCwd(fromTerminalId).catch(() => null);
      if (cwd) setCwd(id, cwd);
    }
    await saveTerminal({
      id,
      workspaceId: workspace.id,
      connectionId: null,
      title: "Terminal",
      startupTasks: [],
      sortOrder: 0,
      shell: "",
    });
  };

  /**
   * Both sidebar drags produce a terminal; only the source of its settings
   * differs. A preset is copied in full (connection, shell, startup tasks and
   * variables), a connection contributes just itself.
   */
  const draggedItem = (dataTransfer: DataTransfer | null) => {
    if (isConnectionDrag(dataTransfer)) {
      const c = getDraggedConnection();
      if (c) return { name: c.name, create: (id: string) => createTerminalForConnection(c, id) };
    }
    if (isPresetDrag(dataTransfer)) {
      const p = getDraggedPreset();
      if (p) return { name: p.name, create: (id: string) => terminalFromPreset(p, workspace.id, id) };
    }
    return undefined;
  };

  const onExternalDrag = (event: React.DragEvent<HTMLElement>) => {
    const item = draggedItem(event.dataTransfer);
    if (!item) return undefined;
    const id = crypto.randomUUID();
    return {
      json: { type: "tab", id, name: item.name, component: "terminal" },
      onDrop: (node?: unknown) => {
        // node is undefined when the drag was cancelled, so nothing was added.
        if (!node) return;
        item.create(id).catch(() => {
          model.doAction(Actions.deleteTab(id));
        });
      },
    };
  };

  // The layout is not mounted while the workspace is empty, so the empty state
  // takes the drop itself; the sync effect then places the tab.
  const onEmptyDrop = (event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDropTarget(false);
    draggedItem(event.dataTransfer)?.create(crypto.randomUUID());
  };

  /** Right-clicking a tab exposes the per-terminal actions. */
  const tabContextMenu: NodeMouseEvent = (node, event) => {
    if (node.getType() !== "tab") return;
    const t = terminals?.find((x) => x.id === node.getId());
    if (!t) return;
    // "Close others" acts on the pane the tab lives in, not the whole
    // workspace, so terminals split off into other panes are left alone.
    const siblingIds = new Set(
      (node.getParent()?.getChildren() ?? [])
        .filter((c) => c.getType() === "tab" && c.getId() !== t.id)
        .map((c) => c.getId())
    );
    const siblings = (terminals ?? []).filter((x) => siblingIds.has(x.id));
    event.preventDefault();
    setMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: "Reconnect", onClick: () => reconnect(t.id) },
        { label: "Terminal settings…", onClick: () => setEditing(t) },
        // Hidden entirely without Claude Code: an action that can only ever say
        // "nothing here" is worse than no action at all.
        ...(claudeInstalled
          ? [{ label: "Resume Claude session…", onClick: () => setResuming(t) }]
          : []),
        { separator: true, label: "" },
        // Closing goes through the store, not Actions.deleteTab: a direct
        // model action bypasses the Layout's onAction, so the terminal def
        // would survive and the sync effect would put the tab straight back.
        ...(siblings.length > 0
          ? [
              {
                label: `Close others (${siblings.length})`,
                danger: true,
                onClick: () => siblings.forEach((x) => removeTerminal(x)),
              },
            ]
          : []),
        {
          label: "Close terminal",
          danger: true,
          onClick: () => removeTerminal(t),
        },
      ],
    });
  };

  /**
   * The overflow button lists every terminal of the tabset, not just the ones
   * that don't fit, so it doubles as a switcher. The heading row has no action
   * and only dismisses the menu, which makes closing it a matter of clicking
   * the same spot twice.
   */
  const overflowMenu: ShowOverflowMenuCallback = (node, event) => {
    const tabs = node.getChildren().filter((c) => c.getType() === "tab") as TabNode[];
    const selectedId = node.getSelectedNode()?.getId();
    event.preventDefault();
    setMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: "All terminals", muted: true },
        { separator: true, label: "" },
        ...tabs.map((t) => ({
          label: t.getName(),
          selected: t.getId() === selectedId,
          onClick: () => model.doAction(Actions.selectTab(t.getId())),
        })),
      ],
    });
  };

  /**
   * Dragging a tab to another pane shows the same faded, dashed ghost as
   * dragging a connection out of the sidebar; the styling lives on
   * .flexlayout__drag_rect, so only the contents are supplied here.
   */
  const renderDragRect: DragRectRenderCallback = (content, node) => {
    if (!node || node.getType() !== "tab") return content;
    return (
      <>
        <span className="dot" />
        {(node as TabNode).getName()}
      </>
    );
  };

  /**
   * FlexLayout (0.10.1) only remeasures a tabstrip on resize, scroll or a
   * change of selected tab, so closing a tab that was none of those leaves the
   * overflow button showing and the sticky + button docked to the far right.
   * A scroll event is the cheapest way to make it measure again.
   */
  const remeasureTabStrips = () => {
    requestAnimationFrame(() => {
      document
        .querySelectorAll(".flexlayout__tabset_tabbar_inner")
        .forEach((el) => el.dispatchEvent(new Event("scroll")));
    });
  };

  /**
   * Publish this workspace's grid to the quick search palette. Re-registered
   * on every render so the handle always closes over the current callbacks.
   */
  useEffect(() =>
    registerWorkspace(workspace.id, {
      focusTerminal: (terminalId) => {
        const node = model.getNodeById(terminalId);
        if (!node) return;
        model.doAction(Actions.selectTab(terminalId));
        const parent = node.getParent();
        if (parent instanceof TabSetNode) {
          model.doAction(Actions.setActiveTabset(parent.getId()));
        }
      },
      openTerminal: () => {
        const selected = model.getActiveTabset()?.getSelectedNode() as
          | TabNode
          | undefined;
        openLocalTerminal(selected?.getId());
      },
      closeActiveTerminal: () => {
        // No active tabset yet (nothing has been clicked since the workspace
        // opened) still leaves a selected tab to close in the first pane.
        const selected = (model.getActiveTabset() ?? model.getFirstTabSet())
          ?.getSelectedNode() as TabNode | undefined;
        const t = terminals?.find((x) => x.id === selected?.getId());
        if (!t) return false;
        // Same reason as the context menu: the store owns removal, a bare
        // deleteTab would leave the def behind and the tab would come back.
        removeTerminal(t);
        remeasureTabStrips();
        return true;
      },
    })
  );

  const onAction = (action: Action) => {
    if (action.type === Actions.DELETE_TAB) {
      const t = terminals?.find((x) => x.id === action.data.node);
      if (t) removeTerminal(t);
      remeasureTabStrips();
    }
    // Looking at the tab is what the error dot was waiting for. It reports
    // "something failed while you were elsewhere", so clearing it on arrival is
    // the whole point — and it keeps a chatty non-zero exit (a fruitless grep,
    // a failing test loop) from souring the indicator permanently.
    if (action.type === Actions.SELECT_TAB) {
      clearError(action.data.tabNode);
      clearAttention(action.data.tabNode);
      // Bringing the tab forward is the user looking at the result, whether or
      // not they then click into the terminal itself.
      clearFinished(action.data.tabNode);
      clearActivity(action.data.tabNode);
    }
    return action;
  };

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: visible ? "block" : "none",
      }}
    >
      {(!terminals || terminals.length === 0) && (
        <div
          className={`empty-state${dropTarget ? " drop-target" : ""}`}
          onDragOver={(e) => {
            if (!isConnectionDrag(e.dataTransfer) && !isPresetDrag(e.dataTransfer)) return;
            e.preventDefault();
            // "move" matches the effectAllowed set in the sidebar; "copy"
            // would put a + badge on the cursor.
            e.dataTransfer.dropEffect = "move";
            setDropTarget(true);
          }}
          onDragLeave={() => setDropTarget(false)}
          onDrop={onEmptyDrop}
        >
          <h2>{workspace.name}</h2>
          <span>No terminals yet, or drag a connection or preset here.</span>
          <button className="btn primary" onClick={() => openLocalTerminal()}>
            + Add terminal
          </button>
        </div>
      )}
      {terminals && terminals.length > 0 && (
        <Layout
          model={model}
          factory={factory}
          // Resize the panes (and their terminals) live while dragging a
          // splitter, instead of only moving the splitter line.
          realtimeResize
          onExternalDrag={onExternalDrag}
          onAction={onAction}
          onModelChange={(m) => saveLayout(workspace.id, m.toJson())}
          onContextMenu={tabContextMenu}
          onShowOverflowMenu={overflowMenu}
          onRenderDragRect={renderDragRect}
          icons={{
            more: (node) => (
              <TerminalCountIcon
                count={node.getChildren().filter((c) => c.getType() === "tab").length}
              />
            ),
          }}
          onRenderTab={(node, renderValues) => {
            const id = node.getId();
            const status = effectiveStatus(id, statuses[id]?.status ?? "idle", {
              agents,
              heuristics,
              finished,
              activity,
            });
            const hint =
              agentTooltip(agents[id], heuristics[id], finished[id]) ??
              statuses[id]?.message ??
              (activity[id] ? "New output since you last looked" : undefined);
            renderValues.leading = (
              <span className="tab-leading">
                <span className={`status-dot ${status}`} title={hint} />
              </span>
            );
            renderValues.content = <TabTitle name={node.getName()} />;
            // Trailing, before the close button: the badge names the tab's
            // occupant, which reads after the title rather than ahead of it.
            if (hasClaude(id, agents, heuristics)) {
              renderValues.buttons.push(
                <ClaudeMark key="claude" title={hint ?? "Claude is open"} />
              );
            }
          }}
          onRenderTabSet={(node, renderValues) => {
            // Reconnect and settings live in the tab's context menu; the
            // tabstrip only keeps the sticky + button.
            const selected =
              node.getType() === "tabset"
                ? ((node as TabSetNode).getSelectedNode() as TabNode | undefined)
                : undefined;
            renderValues.stickyButtons.push(
              <button
                key="add"
                className="tabset-btn"
                title="New terminal"
                onClick={() => openLocalTerminal(selected?.getId(), node.getId())}
              >
                ＋
              </button>
            );
          }}
        />
      )}
      {menu && <ContextMenu {...menu} onClose={() => setMenu(null)} />}
      {resuming && (
        <ClaudeSessionsDialog
          terminalId={resuming.id}
          onClose={() => setResuming(null)}
        />
      )}
      {editing && (
        <TerminalDialog
          workspaceId={workspace.id}
          terminal={editing}
          onClose={() => setEditing(null)}
          onSaved={(t) => {
            const node = model.getNodeById(t.id);
            if (node) model.doAction(Actions.renameTab(t.id, tabName(t)));
          }}
        />
      )}
    </div>
  );
}
