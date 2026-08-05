import { useMemo, useRef, useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";

import { CONNECTION_DRAG_TYPE, setDraggedConnection } from "../../lib/dragConnection";
import { PRESET_DRAG_TYPE, setDraggedPreset } from "../../lib/dragPreset";
import { ipc } from "../../lib/ipc";
import { openConnectionTerminal } from "../../lib/openConnection";
import { openPresetTerminal } from "../../lib/openPreset";
import { tagColor } from "../../lib/tags";
import { noAutoAssist } from "../../lib/inputProps";
import type { Connection, Preset, Workspace } from "../../lib/types";
import { useAgentsStore } from "../../stores/agents";
import { useConnectionsStore } from "../../stores/connections";
import { usePresetsStore } from "../../stores/presets";
import { useSettingsStore } from "../../stores/settings";
import { useTerminalsStore, workspaceStatus } from "../../stores/terminals";
import { useWorkspacesStore } from "../../stores/workspaces";
import { ConfirmDialog, type ConfirmRequest } from "../common/ConfirmDialog";
import { ContextMenu, MenuItem } from "../common/ContextMenu";
import { QUICK_SEARCH_HINT } from "../common/QuickSearch";
import { ScrollArea } from "../common/ScrollArea";
import { TagChip } from "../common/TagChip";
import { ConnectionDialog } from "../dialogs/ConnectionDialog";
import { PresetDialog } from "../dialogs/PresetDialog";
import { WorkspaceDialog } from "../dialogs/WorkspaceDialog";

/** Section heading glyphs; the terminal one matches the tabstrip overflow icon. */
function WorkspacesIcon() {
  return (
    <svg viewBox="0 -960 960 960" width="14" height="14" fill="currentColor" aria-hidden>
      <path d="M160-440v-295.38q0-27.62 18.5-46.12Q197-800 224.62-800H440v360H160Zm240-40Zm120-320h215.38q27.62 0 46.12 18.5Q800-763 800-735.13V-600H520v-200Zm0 640v-360h280v295.38q0 27.62-18.5 46.12Q763-160 735.38-160H520ZM160-360h280v200H224.62q-27.62 0-46.12-18.5Q160-197 160-224.87V-360Zm240 40Zm160-320Zm0 160Zm-360 0h200v-280H224.62q-10.77 0-17.7 6.92-6.92 6.93-6.92 17.7V-480Zm360-160h200v-95.38q0-10.77-6.92-17.7-6.93-6.92-17.7-6.92H560v120Zm0 160v280h175.38q10.77 0 17.7-6.92 6.92-6.93 6.92-17.7V-480H560ZM200-320v95.38q0 10.77 6.92 17.7 6.93 6.92 17.7 6.92H400v-120H200Z" />
    </svg>
  );
}

function ConnectionsIcon() {
  return (
    <svg viewBox="0 -960 960 960" width="14" height="14" fill="currentColor" aria-hidden>
      <path d="M55.39-150.77v-60h849.22v60H55.39Zm116.92-100q-30.31 0-51.31-21-21-21-21-51.31v-415.38q0-30.31 21-51.31 21-21 51.31-21h615.38q30.31 0 51.31 21 21 21 21 51.31v415.38q0 30.31-21 51.31-21 21-51.31 21H172.31Zm0-60h615.38q4.62 0 8.46-3.84 3.85-3.85 3.85-8.47v-415.38q0-4.62-3.85-8.46-3.84-3.85-8.46-3.85H172.31q-4.62 0-8.46 3.85-3.85 3.84-3.85 8.46v415.38q0 4.62 3.85 8.47 3.84 3.84 8.46 3.84Zm-12.31 0v-440 440Z" />
    </svg>
  );
}

function PresetsIcon() {
  return (
    <svg viewBox="0 -960 960 960" width="14" height="14" fill="currentColor" aria-hidden>
      <path d="M584.62-120q-27.62 0-46.12-18.5Q520-157 520-184.62v-150.76q0-27.62 18.5-46.12Q557-400 584.62-400h230.76q27.62 0 46.12 18.5Q880-363 880-335.38v150.76q0 27.62-18.5 46.12Q843-120 815.38-120H584.62Zm0-40h230.76q10.77 0 17.7-6.92 6.92-6.93 6.92-17.7v-150.76q0-10.77-6.92-17.7-6.93-6.92-17.7-6.92H584.62q-10.77 0-17.7 6.92-6.92 6.93-6.92 17.7v150.76q0 10.77 6.92 17.7 6.93 6.92 17.7 6.92ZM443.85-386q-28.39-10.92-46.66-36.15-18.27-25.23-18.27-57.85 0-41.85 29.08-70.92Q437.08-580 478.92-580q40.48 0 69.09 27.54 28.61 27.54 30.91 67.08h-9.69q-8.69.76-15.88 1.88-7.2 1.12-14.66 3.35v-1.62Q537.92-506 520.54-523q-17.39-17-42-17-24.62 0-42.12 17.5-17.5 17.5-17.5 42.25 0 20.79 11.73 36.14 11.73 15.34 29.43 21.11-5.85 8.54-9.77 17.54-3.93 9-6.46 19.46Zm35.07-94Zm-73.54 360-14.46-115.69q-19.15-5.77-41.42-18.16-22.27-12.38-37.88-26.53L204.92-235l-74.61-130 92.23-69.54q-1.77-10.84-2.92-22.34-1.16-11.5-1.16-21.97 0-10.46 1.16-21.57 1.15-11.12 2.92-25.04L130.31-595l74.61-128.46 105.93 44.61q17.92-14.92 38.77-26.92 20.84-12 40.53-18.54L405.38-840h149.24l14.46 116.46q23 8.08 40.65 18.54 17.65 10.46 36.35 26.15l109-44.61L829.69-595l-95.31 71.85q3.31 12.38 3.7 22.73.38 10.34.38 20.42h-41.54q-.23-17.46-2.23-30.81-2-13.34-6-27.88L777.77-606 738-674l-102.08 42.77q-18.15-19.92-47.73-37.35-29.57-17.42-55.96-23.11L520-800h-79.77l-12.46 107.54q-30.23 6.46-55.58 20.81-25.34 14.34-50.42 39.42L222-674l-39.77 68L269-541.23q-5 13.46-7 29.23t-2 32.56q0 15.81 2 30.62 2 14.82 6.23 29.05l-86 65.77L222-286l99-42q24 24.23 54.38 40.08 30.39 15.84 64.62 23.15V-120h-34.62Zm73.54-360ZM700-260Z" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg
      className="sidebar-search-icon"
      viewBox="0 0 24 24"
      width="14"
      height="14"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      aria-hidden
    >
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  );
}

/** Points the way the panel would move, so it reads as its own action. */
function ChevronIcon({ dir }: { dir: "left" | "right" }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="14"
      height="14"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d={dir === "left" ? "m14 7-5 5 5 5" : "m10 7 5 5-5 5"} />
    </svg>
  );
}

/**
 * Alphabetical throughout the sidebar: the lists come back in insertion order,
 * which is the one order the user cannot predict from the row itself. Every
 * caller sorts a filter() copy, so the stores' own arrays are never touched.
 */
const byName = <T extends { name: string }>(a: T, b: T) =>
  a.name.localeCompare(b.name);

interface Menu {
  x: number;
  y: number;
  items: MenuItem[];
}

interface Props {
  onOpenQuickSearch: () => void;
}

export function Sidebar({ onOpenQuickSearch }: Props) {
  const workspaces = useWorkspacesStore((s) => s.workspaces);
  const wsSearch = useWorkspacesStore((s) => s.search);
  const setWsSearch = useWorkspacesStore((s) => s.setSearch);
  const openWorkspace = useWorkspacesStore((s) => s.open);
  const activeId = useWorkspacesStore((s) => s.activeId);
  const removeWorkspace = useWorkspacesStore((s) => s.remove);
  const duplicateWorkspace = useWorkspacesStore((s) => s.duplicate);

  const connections = useConnectionsStore((s) => s.connections);
  const removeConnection = useConnectionsStore((s) => s.remove);
  const duplicateConnection = useConnectionsStore((s) => s.duplicate);

  const presets = usePresetsStore((s) => s.presets);
  const removePreset = usePresetsStore((s) => s.remove);
  const duplicatePreset = usePresetsStore((s) => s.duplicate);

  const byWorkspace = useTerminalsStore((s) => s.byWorkspace);
  const statuses = useTerminalsStore((s) => s.statuses);
  const agents = useAgentsStore((s) => s.byTerminal);
  const heuristics = useAgentsStore((s) => s.byTerminalHeuristic);
  const finished = useAgentsStore((s) => s.finished);
  const activity = useTerminalsStore((s) => s.activity);

  const collapsedFolders = useSettingsStore((s) => s.ui.collapsedFolders);
  const collapsed = useSettingsStore((s) => s.ui.sidebarCollapsed ?? false);
  const setUi = useSettingsStore((s) => s.setUi);
  const tagColors = useSettingsStore((s) => s.tagColors);

  // The element handed to setDragImage() has to stay in the document while the
  // drag runs (the browser rasterizes it on the next frame), so it is parked
  // off-screen and torn down on dragend.
  const dragImageRef = useRef<HTMLDivElement | null>(null);

  const makeDragImage = (name: string) => {
    dragImageRef.current?.remove();
    const el = document.createElement("div");
    el.className = "connection-drag-image";
    const dot = document.createElement("span");
    dot.className = "dot";
    el.append(dot, document.createTextNode(name));
    document.body.appendChild(el);
    dragImageRef.current = el;
    return el;
  };

  const [menu, setMenu] = useState<Menu | null>(null);
  const [confirming, setConfirming] = useState<ConfirmRequest | null>(null);
  const [editWorkspace, setEditWorkspace] = useState<Workspace | null>(null);
  const [creatingWorkspace, setCreatingWorkspace] = useState(false);
  const [editConnection, setEditConnection] = useState<Connection | null>(null);
  const [creatingConnection, setCreatingConnection] = useState(false);
  const [editPreset, setEditPreset] = useState<Preset | null>(null);
  const [creatingPreset, setCreatingPreset] = useState(false);

  const filteredWorkspaces = useMemo(() => {
    // The rail has no search field, so a query left behind before collapsing
    // would hide workspaces with nothing on screen explaining why.
    const q = collapsed ? "" : wsSearch.toLowerCase();
    return workspaces
      .filter(
        (w) =>
          w.name.toLowerCase().includes(q) ||
          w.tags.some((t) => t.toLowerCase().includes(q))
      )
      .sort(byName);
  }, [workspaces, wsSearch, collapsed]);

  const filteredConnections = useMemo(() => {
    const q = wsSearch.toLowerCase();
    return connections
      .filter(
        (c) =>
          c.name.toLowerCase().includes(q) ||
          c.host.toLowerCase().includes(q) ||
          c.tags.some((t) => t.toLowerCase().includes(q))
      )
      .sort(byName);
  }, [connections, wsSearch]);

  const filteredPresets = useMemo(() => {
    const q = wsSearch.toLowerCase();
    return presets
      .filter(
        (p) =>
          p.name.toLowerCase().includes(q) ||
          p.tags.some((t) => t.toLowerCase().includes(q))
      )
      .sort(byName);
  }, [presets, wsSearch]);

  const connectionFolders = useMemo(() => {
    const map = new Map<string, Connection[]>();
    for (const c of filteredConnections) {
      const key = c.folder || "";
      map.set(key, [...(map.get(key) ?? []), c]);
    }
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [filteredConnections]);

  // While searching, every folder opens: a hidden match reads as "no results".
  const isCollapsed = (folder: string) =>
    !wsSearch && collapsedFolders.includes(folder);

  const toggleFolder = (folder: string) =>
    setUi({
      collapsedFolders: collapsedFolders.includes(folder)
        ? collapsedFolders.filter((f) => f !== folder)
        : [...collapsedFolders, folder],
    });

  const exportWorkspace = async (w: Workspace) => {
    const json = await ipc.workspaceExport(w.id);
    const path = await saveFileDialog({
      defaultPath: `${w.name.toLowerCase().replace(/\s+/g, "-")}.workspace.json`,
      filters: [{ name: "Workspace", extensions: ["json"] }],
    });
    if (path) await writeTextFile(path, json);
  };

  const workspaceMenu = (e: React.MouseEvent, w: Workspace) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Open", onClick: () => openWorkspace(w.id) },
        { label: "Edit…", onClick: () => setEditWorkspace(w) },
        { label: "Duplicate", onClick: () => duplicateWorkspace(w.id) },
        { label: "Export…", onClick: () => exportWorkspace(w) },
        { separator: true, label: "" },
        {
          label: "Delete",
          danger: true,
          onClick: () =>
            setConfirming({
              title: "Delete workspace",
              message: `Delete workspace "${w.name}"?`,
              detail: byWorkspace[w.id]?.length
                ? `Its ${byWorkspace[w.id].length} terminal(s) and its variables are deleted too.`
                : "Its variables are deleted too.",
              onConfirm: () => removeWorkspace(w.id),
            }),
        },
      ],
    });
  };

  const connectionMenu = (e: React.MouseEvent, c: Connection) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Open terminal", onClick: () => openConnectionTerminal(c) },
        { label: "Edit…", onClick: () => setEditConnection(c) },
        { label: "Duplicate", onClick: () => duplicateConnection(c.id) },
        { separator: true, label: "" },
        {
          label: "Delete",
          danger: true,
          onClick: () => {
            const affected = presets.filter((p) => p.connectionId === c.id);
            setConfirming({
              title: "Delete connection",
              message: `Delete connection "${c.name}"?`,
              detail: affected.length
                ? `${affected.length} preset(s) built on it are deleted too: ${affected
                    .map((p) => p.name)
                    .join(", ")}.`
                : undefined,
              onConfirm: () => removeConnection(c.id),
            });
          },
        },
      ],
    });
  };

  const presetMenu = (e: React.MouseEvent, p: Preset) => {
    e.preventDefault();
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Open terminal", onClick: () => openPresetTerminal(p) },
        { label: "Edit…", onClick: () => setEditPreset(p) },
        { label: "Duplicate", onClick: () => duplicatePreset(p.id) },
        { separator: true, label: "" },
        {
          label: "Delete",
          danger: true,
          onClick: () =>
            setConfirming({
              title: "Delete preset",
              message: `Delete preset "${p.name}"?`,
              detail: "Terminals already opened from it are not affected.",
              onConfirm: () => removePreset(p.id),
            }),
        },
      ],
    });
  };

  const workspaceDot = (w: Workspace) =>
    workspaceStatus(byWorkspace[w.id], statuses, {
      agents,
      heuristics,
      finished,
      activity,
    });

  const toggleCollapsed = () => setUi({ sidebarCollapsed: !collapsed });

  // Menus and dialogs belong to both layouts: the rail keeps the workspace
  // context menu, so it needs the same editors behind it.
  const overlays = (
    <>
      {menu && <ContextMenu {...menu} onClose={() => setMenu(null)} />}
      {confirming && (
        <ConfirmDialog request={confirming} onClose={() => setConfirming(null)} />
      )}
      {(creatingWorkspace || editWorkspace) && (
        <WorkspaceDialog
          workspace={editWorkspace}
          onClose={() => {
            setCreatingWorkspace(false);
            setEditWorkspace(null);
          }}
        />
      )}
      {(creatingConnection || editConnection) && (
        <ConnectionDialog
          connection={editConnection}
          onClose={() => {
            setCreatingConnection(false);
            setEditConnection(null);
          }}
        />
      )}
      {(creatingPreset || editPreset) && (
        <PresetDialog
          preset={editPreset}
          onClose={() => {
            setCreatingPreset(false);
            setEditPreset(null);
          }}
        />
      )}
    </>
  );

  if (collapsed) {
    return (
      <div className="sidebar collapsed">
        {/* No field fits at this width, so search becomes the one thing it can
            still be here: the way into the go-to-anything palette. */}
        <button
          className="rail-btn"
          title={`Go to anything (${QUICK_SEARCH_HINT})`}
          onClick={onOpenQuickSearch}
        >
          <SearchIcon />
        </button>
        <div className="rail-divider" />

        <ScrollArea className="rail-content">
          {filteredWorkspaces.map((w) => (
            <div
              key={w.id}
              className={`rail-item${w.id === activeId ? " active" : ""}`}
              onClick={() => openWorkspace(w.id)}
              onContextMenu={(e) => workspaceMenu(e, w)}
              title={w.tags.length ? `${w.name}\n${w.tags.join(", ")}` : w.name}
              // The pill takes the colour of its first tag, which is the only
              // colour a workspace has now; untagged ones fall back to the accent.
              style={
                w.tags.length
                  ? ({ "--rail-color": tagColor(w.tags[0], tagColors) } as React.CSSProperties)
                  : undefined
              }
            >
              {byWorkspace[w.id]?.length ? (
                <span className="rail-count">{byWorkspace[w.id].length}</span>
              ) : null}
              <span className="rail-name">{w.name}</span>
              <span className={`status-dot ${workspaceDot(w)}`} />
            </div>
          ))}
          <button
            className="rail-btn rail-add"
            title="New workspace"
            onClick={() => setCreatingWorkspace(true)}
          >
            ＋
          </button>
        </ScrollArea>

        <button
          className="rail-btn rail-toggle"
          title="Expand sidebar"
          onClick={toggleCollapsed}
        >
          <ChevronIcon dir="right" />
        </button>

        {overlays}
      </div>
    );
  }

  return (
    <div className="sidebar">
      <div className="sidebar-search">
        <SearchIcon />
        <input
          {...noAutoAssist}
          placeholder="Search…"
          value={wsSearch}
          onChange={(e) => setWsSearch(e.target.value)}
        />
        {/* The field filters the lists below; the badge is the way into the
            go-to-anything palette, which the same combo also opens. */}
        <button
          className="sidebar-search-hint"
          title="Go to anything"
          onClick={onOpenQuickSearch}
        >
          {QUICK_SEARCH_HINT}
        </button>
      </div>

      <ScrollArea className="sidebar-content">
        <div className="sidebar-section-title">
          <span className="section-label">
            <WorkspacesIcon />
            Workspaces
          </span>
          <button title="New workspace" onClick={() => setCreatingWorkspace(true)}>＋</button>
        </div>
        {filteredWorkspaces.map((w) => (
          <div
            key={w.id}
            className={`sidebar-item${w.id === activeId ? " active" : ""}${
              w.tags.length ? " has-tags" : ""
            }`}
            onClick={() => openWorkspace(w.id)}
            onContextMenu={(e) => workspaceMenu(e, w)}
            title={w.tags.length ? `${w.name}\n${w.tags.join(", ")}` : w.name}
          >
            {/* Same two-line shape as presets and connections: chips get their
                own line so they never squeeze the name's ellipsis — or, here,
                push the count pill out of the row. */}
            <span className="item-main">
              <span className={`status-dot ${workspaceDot(w)}`} />
              <span className="item-name">{w.name}</span>
              {/* Only workspaces that hold terminals get the badge — an empty
                  pill would read as a count of zero that is worth looking at. */}
              {byWorkspace[w.id]?.length ? (
                <span className="item-sub">{byWorkspace[w.id].length}</span>
              ) : null}
            </span>
            {w.tags.length > 0 && (
              <span className="item-tags">
                {w.tags.map((t) => (
                  <TagChip key={t} tag={t} />
                ))}
              </span>
            )}
          </div>
        ))}
        {filteredWorkspaces.length === 0 && (
          <div className="sidebar-item" style={{ color: "var(--text-muted)" }}>
            No workspaces
          </div>
        )}

        <div className="sidebar-section-title">
          <span className="section-label">
            <PresetsIcon />
            Presets
          </span>
          <button title="New preset" onClick={() => setCreatingPreset(true)}>＋</button>
        </div>
        {filteredPresets.map((p) => (
          <div
            key={p.id}
            className={`sidebar-item${p.tags.length ? " has-tags" : ""}`}
            draggable
            onDragStart={(e) => {
              setDraggedPreset(p);
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData(PRESET_DRAG_TYPE, p.id);
              e.dataTransfer.setData("text/plain", p.name);
              e.dataTransfer.setDragImage(makeDragImage(p.name), 16, 18);
            }}
            onDragEnd={() => {
              setDraggedPreset(null);
              dragImageRef.current?.remove();
              dragImageRef.current = null;
            }}
            onDoubleClick={() => openPresetTerminal(p)}
            onContextMenu={(e) => presetMenu(e, p)}
            title={p.name}
          >
            <span className="item-main">
              <span className="dot" />
              <span className="item-name">{p.name}</span>
            </span>
            {p.tags.length > 0 && (
              <span className="item-tags">
                {p.tags.map((t) => (
                  <TagChip key={t} tag={t} />
                ))}
              </span>
            )}
          </div>
        ))}
        {filteredPresets.length === 0 && (
          <div className="sidebar-item" style={{ color: "var(--text-muted)" }}>
            No presets
          </div>
        )}

        <div className="sidebar-section-title">
          <span className="section-label">
            <ConnectionsIcon />
            Connections
          </span>
          <button title="New connection" onClick={() => setCreatingConnection(true)}>＋</button>
        </div>
        {connectionFolders.map(([folder, list]) => (
          <div key={folder || "_root"}>
            {folder && (
              <div
                className="sidebar-folder"
                onClick={() => toggleFolder(folder)}
                title={isCollapsed(folder) ? "Expand" : "Collapse"}
              >
                <span className="folder-caret">
                  {isCollapsed(folder) ? "▸" : "▾"}
                </span>
                {folder}
                <span className="folder-count">{list.length}</span>
              </div>
            )}
            {!isCollapsed(folder) && list.map((c) => (
              <div
                key={c.id}
                className={`sidebar-item${c.tags.length ? " has-tags" : ""}`}
                draggable
                onDragStart={(e) => {
                  setDraggedConnection(c);
                  // "move" (not "copy") so no platform + badge rides along;
                  // it also matches the dropEffect FlexLayout sets on hover.
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData(CONNECTION_DRAG_TYPE, c.id);
                  e.dataTransfer.setData("text/plain", c.name);
                  e.dataTransfer.setDragImage(makeDragImage(c.name), 16, 18);
                }}
                onDragEnd={() => {
                  setDraggedConnection(null);
                  dragImageRef.current?.remove();
                  dragImageRef.current = null;
                }}
                onDoubleClick={() => openConnectionTerminal(c)}
                onContextMenu={(e) => connectionMenu(e, c)}
                title={`${c.name}\n${c.username}@${c.host}:${c.port}`}
                style={folder ? { paddingLeft: 24 } : undefined}
              >
                <span className="item-main">
                  <span className="dot" />
                  <span className="item-name">{c.name}</span>
                </span>
                {c.tags.length > 0 && (
                  <span className="item-tags">
                    {c.tags.map((t) => (
                      <TagChip key={t} tag={t} />
                    ))}
                  </span>
                )}
              </div>
            ))}
          </div>
        ))}
        {filteredConnections.length === 0 && (
          <div className="sidebar-item" style={{ color: "var(--text-muted)" }}>
            No connections
          </div>
        )}
      </ScrollArea>

      <div className="sidebar-footer">
        <button
          className="rail-btn rail-toggle"
          title="Collapse sidebar"
          onClick={toggleCollapsed}
        >
          <ChevronIcon dir="left" />
        </button>
      </div>

      {overlays}
    </div>
  );
}
