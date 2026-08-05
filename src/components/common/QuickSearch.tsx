import { useEffect, useMemo, useRef, useState } from "react";

import { openConnectionTerminal } from "../../lib/openConnection";
import { openPresetTerminal } from "../../lib/openPreset";
import { noAutoAssist } from "../../lib/inputProps";
import { useAgentsStore } from "../../stores/agents";
import { useConnectionsStore } from "../../stores/connections";
import { usePresetsStore } from "../../stores/presets";
import { useTerminalsStore, workspaceStatus } from "../../stores/terminals";
import { useWorkspacesStore } from "../../stores/workspaces";

/** ⌘P on macOS, Ctrl+P elsewhere — Ctrl+P is a shell binding on macOS. */
const isMac = navigator.platform.toLowerCase().includes("mac");
export const QUICK_SEARCH_HINT = isMac ? "⌘P" : "Ctrl P";

/**
 * Opens the palette on the quick search combo. Listening in the capture phase
 * keeps the combo away from xterm, which would otherwise send it to the shell.
 */
export function useQuickSearchHotkey(open: () => void) {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== "p" || e.altKey || e.shiftKey) return;
      if (isMac ? !e.metaKey : !e.ctrlKey) return;
      e.preventDefault();
      e.stopPropagation();
      open();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open]);
}

interface Item {
  key: string;
  group: string;
  label: string;
  sub?: string;
  /** Status class for the leading dot. Connections and presets have no live
   *  status of their own, so theirs stays the neutral idle grey. */
  status?: string;
  run: () => void;
}

interface Props {
  onClose: () => void;
}

/** Go-to-anything palette: workspaces, then saved connections, then presets. */
export function QuickSearch({ onClose }: Props) {
  const workspaces = useWorkspacesStore((s) => s.workspaces);
  const activeId = useWorkspacesStore((s) => s.activeId);
  const openWorkspace = useWorkspacesStore((s) => s.open);
  const connections = useConnectionsStore((s) => s.connections);
  const presets = usePresetsStore((s) => s.presets);
  const byWorkspace = useTerminalsStore((s) => s.byWorkspace);
  const statuses = useTerminalsStore((s) => s.statuses);
  const agents = useAgentsStore((s) => s.byTerminal);
  const heuristics = useAgentsStore((s) => s.byTerminalHeuristic);
  const finished = useAgentsStore((s) => s.finished);
  const activity = useTerminalsStore((s) => s.activity);

  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = workspaces.map((w) => ({
      key: `w:${w.id}`,
      group: "Workspaces",
      label: w.name,
      sub:
        w.id === activeId
          ? "Active"
          : byWorkspace[w.id]?.length
            ? `${byWorkspace[w.id].length} terminals`
            : w.tags.join(", ") || undefined,
      status: workspaceStatus(byWorkspace[w.id], statuses, {
        agents,
        heuristics,
        finished,
        activity,
      }),
      run: () => openWorkspace(w.id),
    }));

    // A connection opens as a terminal in the focused pane of the active
    // workspace, exactly like double-clicking it in the sidebar.
    for (const c of connections) {
      out.push({
        key: `c:${c.id}`,
        group: "Connections",
        label: c.name,
        sub: `${c.username}@${c.host}`,
        run: () => openConnectionTerminal(c),
      });
    }

    // A preset opens the same way, with its settings copied onto the terminal.
    for (const p of presets) {
      out.push({
        key: `p:${p.id}`,
        group: "Presets",
        label: p.name,
        sub: p.tags.join(", ") || undefined,
        run: () => openPresetTerminal(p),
      });
    }
    return out;
  }, [workspaces, connections, presets, byWorkspace, statuses, agents, heuristics, finished, activity, activeId, openWorkspace]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((i) =>
      `${i.label} ${i.sub ?? ""}`.toLowerCase().includes(q)
    );
  }, [items, query]);

  // A shrinking result list must not leave the cursor past its end.
  useEffect(() => setIndex(0), [query]);

  useEffect(() => {
    listRef.current
      ?.querySelector(".quick-search-item.active")
      ?.scrollIntoView({ block: "nearest" });
  }, [index, filtered]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    onClose();
    item.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (filtered.length === 0) return;
      const step = e.key === "ArrowDown" ? 1 : -1;
      setIndex((i) => (i + step + filtered.length) % filtered.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(filtered[index]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  let lastGroup = "";

  return (
    <div className="quick-search-backdrop" onMouseDown={onClose}>
      <div className="quick-search" onMouseDown={(e) => e.stopPropagation()}>
        <input
          {...noAutoAssist}
          autoFocus
          className="quick-search-input"
          placeholder="Go to workspace, connection or preset…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="quick-search-list" ref={listRef}>
          {filtered.length === 0 && (
            <div className="quick-search-empty">No matches</div>
          )}
          {filtered.map((item, i) => {
            const heading = item.group !== lastGroup ? item.group : null;
            lastGroup = item.group;
            return (
              <div key={item.key}>
                {heading && <div className="quick-search-group">{heading}</div>}
                <div
                  className={`quick-search-item${i === index ? " active" : ""}`}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => choose(item)}
                >
                  <span className={`status-dot ${item.status ?? "idle"}`} />
                  <span className="quick-search-label">{item.label}</span>
                  {item.sub && <span className="quick-search-sub">{item.sub}</span>}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
