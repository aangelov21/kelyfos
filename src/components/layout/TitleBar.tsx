import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import logoUrl from "../../assets/kelyfos-logo.png";

interface Props {
  onOpenSettings: () => void;
}

const platform = navigator.platform.toLowerCase();
/** Only macOS overlays its window controls on the bar and needs room for them. */
const isMac = platform.includes("mac");
/** Windows runs with decorations off, so the buttons are ours to draw. */
const isWindows = platform.includes("win");

/**
 * The window's own title bar, on both platforms, because neither shows a native
 * one: macOS is configured with titleBarStyle "Overlay" and a hidden title,
 * Windows has its decorations removed in lib.rs.
 *
 * What differs is who draws the buttons. macOS still draws the traffic lights
 * itself and only needs the space they sit in reserved — see the padding on
 * .titlebar.mac. Windows draws nothing, so the three below stand in for the
 * caption buttons and have to behave like them.
 *
 * data-tauri-drag-region makes the strip draggable, and double-clicking it
 * zooms the window, as a native title bar would. Tauri only treats the element
 * the pointer actually landed on as a handle, which is why the logo opts in
 * explicitly and the buttons, by saying nothing, stay clickable.
 */
export function TitleBar({ onOpenSettings }: Props) {
  return (
    <div
      className={`titlebar${isMac ? " mac" : ""}${isWindows ? " win" : ""}`}
      data-tauri-drag-region
    >
      <img className="titlebar-logo" src={logoUrl} alt="Kelyfos" data-tauri-drag-region />
      <button className="titlebar-btn" title="Settings" onClick={onOpenSettings}>
        <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
          <circle cx="12" cy="12" r="3.2" />
          <path d="M19.4 15a1.6 1.6 0 0 0 .32 1.77l.06.06a1.9 1.9 0 1 1-2.7 2.7l-.05-.06a1.6 1.6 0 0 0-1.78-.32 1.6 1.6 0 0 0-.97 1.47V21a1.9 1.9 0 0 1-3.8 0v-.1a1.6 1.6 0 0 0-1.04-1.46 1.6 1.6 0 0 0-1.77.32l-.06.06a1.9 1.9 0 1 1-2.7-2.7l.06-.06a1.6 1.6 0 0 0 .32-1.77 1.6 1.6 0 0 0-1.47-.98H3a1.9 1.9 0 0 1 0-3.8h.1a1.6 1.6 0 0 0 1.46-1.03 1.6 1.6 0 0 0-.32-1.78l-.06-.05a1.9 1.9 0 1 1 2.7-2.7l.06.06a1.6 1.6 0 0 0 1.77.32H9a1.6 1.6 0 0 0 .97-1.47V3a1.9 1.9 0 1 1 3.8 0v.1a1.6 1.6 0 0 0 .97 1.47 1.6 1.6 0 0 0 1.78-.32l.05-.06a1.9 1.9 0 1 1 2.7 2.7l-.06.06a1.6 1.6 0 0 0-.32 1.77V9a1.6 1.6 0 0 0 1.47.97H21a1.9 1.9 0 0 1 0 3.8h-.1a1.6 1.6 0 0 0-1.47.97Z" />
        </svg>
      </button>
      {isWindows && <WindowControls />}
    </div>
  );
}

/**
 * The Tauri window, or null when the UI is being served on its own — a plain
 * `npm run dev` opened in a browser has no IPC, and `getCurrentWindow` throws
 * rather than returning nothing. Resolved per call rather than once at render:
 * a throw during render takes the whole title bar down with it, and no button
 * here is worth that.
 */
function appWindow() {
  try {
    return getCurrentWindow();
  } catch {
    return null;
  }
}

/**
 * Minimise, maximise/restore and close, in the order and at the metrics Windows
 * uses, because muscle memory for the top-right corner is the whole point of
 * matching it. The middle button swaps its icon with the window's state — a
 * restore glyph on a maximised window is the only cue that the button will
 * shrink it rather than grow it.
 */
function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = appWindow();
    if (!win) return;
    const sync = () => {
      win.isMaximized().then(setMaximized).catch(() => {});
    };
    sync();
    // onResized covers every route into and out of maximised — the button, a
    // double-click on the strip, the keyboard, and aero snap, which no click
    // handler here would ever hear about.
    const pending = win.onResized(sync);
    return () => {
      pending.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  return (
    <div className="window-controls">
      <button
        className="window-control"
        title="Minimise"
        aria-label="Minimise"
        onClick={() => appWindow()?.minimize().catch(() => {})}
      >
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden>
          <path d="M0 5h10" stroke="currentColor" strokeWidth="1" shapeRendering="crispEdges" />
        </svg>
      </button>
      <button
        className="window-control"
        title={maximized ? "Restore" : "Maximise"}
        aria-label={maximized ? "Restore" : "Maximise"}
        onClick={() => appWindow()?.toggleMaximize().catch(() => {})}
      >
        <svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1" shapeRendering="crispEdges" aria-hidden>
          {maximized ? (
            <>
              <rect x="0.5" y="2.5" width="7" height="7" />
              <path d="M2.5 2.5v-2h7v7h-2" />
            </>
          ) : (
            <rect x="0.5" y="0.5" width="9" height="9" />
          )}
        </svg>
      </button>
      <button
        className="window-control close"
        title="Close"
        aria-label="Close"
        onClick={() => appWindow()?.close().catch(() => {})}
      >
        <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden>
          <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1.1" />
        </svg>
      </button>
    </div>
  );
}
