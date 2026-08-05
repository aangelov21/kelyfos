import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";

import { ipc } from "../../lib/ipc";
import { createOutputHighlighter } from "../../lib/outputHighlight";
import { getTheme, resolveTerminalTheme } from "../../lib/themes";
import { useAgentsStore } from "../../stores/agents";
import { useSettingsStore } from "../../stores/settings";
import { useTerminalsStore } from "../../stores/terminals";
import type { TerminalStatus } from "../../lib/types";

/**
 * The font picker stores a single family, but a terminal needs a fallback for
 * glyphs that family lacks (box drawing, powerline). Append the generic
 * monospace keyword unless the value is already a stack ending in one.
 */
function fontStack(family: string): string {
  return /(^|,)\s*(monospace|ui-monospace)\s*$/.test(family)
    ? family
    : `${family}, monospace`;
}

/**
 * Every live terminal's WebGL addon, so an atlas rebuild can reach all of them.
 *
 * The WebGL glyph atlas is *shared between terminals*: the addon keeps a
 * module-level cache keyed by font/colors/DPR, and every terminal with the same
 * settings draws from one texture. `clearTextureAtlas()` wipes that shared
 * texture but only redraws its own terminal — the siblings keep vertex data
 * pointing into the wiped texture, and the next thing that makes one redraw
 * (clicking into it, new output) paints the whole pane as garbled fragments.
 * So a rebuild is only safe done globally: wipe through every addon, and let
 * each one's self-queued full redraw re-rasterise. The wipes run synchronously
 * here while the redraws land next frame, so no redraw can be invalidated by a
 * later wipe. A hidden terminal defers its redraw until it is shown again
 * (xterm pauses rendering via IntersectionObserver), which also covers the
 * old "rebuild owed while hidden" case with no bookkeeping of ours.
 */
const liveWebgls = new Set<{ current: WebglAddon | null }>();

function refreshAtlases() {
  for (const ref of liveWebgls) ref.current?.clearTextureAtlas();
}

/**
 * Get a real "this font is ready" signal for an *installed* font.
 *
 * WebKit loads installed fonts into the canvas rasteriser lazily, silently
 * drawing the fallback in the meantime — the brief wrong-font blink on zoom
 * and on cold start. `document.fonts` has the native answer (`load()` resolves
 * when a font is usable) but only tracks fonts registered in it, which
 * installed fonts never are. So register each chosen family once as a
 * FontFace with a `local()` source: same font data, but now the set knows it
 * and `load()` genuinely waits for it. A family that doesn't exist ends up an
 * errored face, which font matching skips — the stack's fallback still
 * applies and nothing here throws.
 *
 * The face is registered under a *probe alias*, never the real family name.
 * A family with any registered face is resolved from those faces alone —
 * installed faces are shadowed — and a `local()` face carries no weight
 * variants (it defaults to 400 and resolves to the Regular face), so
 * registering "Source Code Pro" itself made every weight render as Regular.
 * The alias loads the same underlying font data, which is all the readiness
 * signal needs, while the terminal keeps matching the untouched installed
 * family with all its weights.
 */
const registeredFaces = new Set<string>();
const probeAlias = (family: string) => `${family} kelyfos-probe`;
async function loadFonts(stack: string, size: number): Promise<void> {
  const families = stack
    .split(",")
    .map((f) => f.trim().replace(/^['"]|['"]$/g, ""))
    .filter((f) => f && !/^(ui-)?(monospace|sans-serif|serif)$/i.test(f));
  for (const fam of families) {
    if (registeredFaces.has(fam)) continue;
    registeredFaces.add(fam);
    try {
      document.fonts.add(new FontFace(probeAlias(fam), `local("${fam}")`));
    } catch {
      /* name FontFace rejects: leave it to CSS matching as before */
    }
  }
  await Promise.allSettled(
    families.map((f) => document.fonts.load(`${size}px "${probeAlias(f)}"`))
  );
}

const atlasRetries: number[] = [];

/**
 * Rebuild now and again a few times over the next seconds.
 *
 * A family the canvas rasteriser has not used yet can take WebKit a moment to
 * make available; glyphs drawn before that are rasterised in the fallback font
 * and then cached under the *correct* config, so they never fix themselves —
 * that was the cold-start "default font", repaired only by anything that
 * happened to rebuild later (workspace switch, reconnect). There is no event to
 * wait on — `document.fonts` only tracks @font-face, not installed fonts — so
 * retry on a short schedule instead. Each pass is just re-rasterising the
 * visible glyphs; invisible when nothing changed.
 */
function refreshAtlasesSoon() {
  while (atlasRetries.length) clearTimeout(atlasRetries.pop());
  refreshAtlases();
  for (const ms of [100, 300, 1200, 3000]) {
    atlasRetries.push(window.setTimeout(refreshAtlases, ms));
  }
}

/** One character per byte, as `atob` and the highlighter both work in. */
function latin1ToBytes(bin: string): Uint8Array {
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

interface Props {
  terminalId: string;
  connectionId: string | null;
  workspaceId: string;
  /** Per-terminal shell override; empty falls back to the global setting. */
  shell?: string;
}

export function XtermView({ terminalId, connectionId, workspaceId, shell }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");

  const terminalSettings = useSettingsStore((s) => s.terminal);
  const themeId = useSettingsStore((s) => s.ui.themeId);
  const accent = useSettingsStore((s) => s.ui.accent);
  const setStatus = useTerminalsStore((s) => s.setStatus);

  // Every terminal is a local shell. A saved connection just gets an `ssh …`
  // line typed into it, so the pane stays usable whether or not that succeeds.
  const connect = () => {
    const term = termRef.current;
    if (!term) return;
    const chosenShell =
      shell || useSettingsStore.getState().terminal.shell || null;
    // Set when this terminal was opened from another one, to start in the same
    // directory; otherwise the backend uses the home directory.
    const cwd = useTerminalsStore.getState().cwds[terminalId] ?? null;
    setStatus(terminalId, { status: "connecting" });
    ipc
      .ptyOpen(
        terminalId,
        connectionId,
        workspaceId,
        chosenShell,
        cwd,
        term.cols,
        term.rows
      )
      .catch((e) => setStatus(terminalId, { status: "error", message: String(e) }));
  };

  // Create the terminal once.
  useEffect(() => {
    if (!hostRef.current) return;
    const s = useSettingsStore.getState().terminal;
    const theme = getTheme(useSettingsStore.getState().ui.themeId);

    // Fire-and-forget: registration alone starts WebKit loading the family,
    // and the warm-up rebuild below repaints whatever the first frames drew
    // before it finished.
    loadFonts(fontStack(s.fontFamily), s.fontSize);
    const term = new Terminal({
      fontFamily: fontStack(s.fontFamily),
      fontSize: s.fontSize,
      fontWeight: s.fontWeight,
      fontWeightBold: s.fontWeightBold,
      lineHeight: s.lineHeight,
      cursorStyle: s.cursorStyle,
      cursorBlink: s.cursorBlink,
      scrollback: 10000,
      allowProposedApi: true,
      theme: resolveTerminalTheme(theme, s, useSettingsStore.getState().ui.accent),
    });
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new WebLinksAddon());
    term.open(hostRef.current);

    // The WebGL renderer draws block-element and box-drawing characters itself
    // rather than taking them from the font (xterm's `customGlyphs`, on by
    // default but inert on the DOM renderer). Without it those glyphs are only
    // as tall as the font's em box, so with a line height above 1 they cannot
    // touch — block art like Claude Code's banner comes out striped, and box
    // drawing shows hairline seams. It also renders faster.
    //
    // Must be loaded after open(): it needs the canvas the terminal creates.
    let webgl: WebglAddon | null = null;
    try {
      webgl = new WebglAddon();
      // A lost context is not recoverable in place. Dropping the addon falls
      // back to the DOM renderer, which looks worse but keeps the terminal
      // working — going blank because a GPU hiccuped would be far worse.
      webgl.onContextLoss(() => {
        webgl?.dispose();
        webgl = null;
        webglRef.current = null;
      });
      term.loadAddon(webgl);
    } catch {
      // No WebGL on this machine (a VM, or a locked-down driver). The DOM
      // renderer is already in place and needs no action.
      webgl = null;
    }
    webglRef.current = webgl;
    liveWebgls.add(webglRef);

    fit.fit();

    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;

    // The atlas filled during open() can predate the family being usable by
    // the rasteriser; the retries in refreshAtlasesSoon replace those glyphs
    // once it is. First pass after a frame, when the initial layout has run.
    const warmup = requestAnimationFrame(refreshAtlasesSoon);

    term.onData((data) => {
      ipc.ptyWrite(terminalId, data).catch(() => {});
    });

    // Which terminal has the keyboard, so a command that fails in front of the
    // user does not also raise a flag telling them it failed.
    const setFocused = useTerminalsStore.getState().setFocused;
    const onFocus = () => {
      setFocused(terminalId);
      useAgentsStore.getState().clearAttention(terminalId);
      useAgentsStore.getState().clearFinished(terminalId);
      useTerminalsStore.getState().clearActivity(terminalId);
    };
    const onBlur = () => {
      if (useTerminalsStore.getState().focusedId === terminalId) setFocused(null);
    };
    term.textarea?.addEventListener("focus", onFocus);
    term.textarea?.addEventListener("blur", onBlur);

    // Fallback signals, for the cases the backend cannot see — chiefly an SSH
    // session, where whatever is running lives on the far end. The bell is how
    // a program asks for the user; the title is how Claude names itself.
    term.onBell(() => {
      const agents = useAgentsStore.getState();
      if (useTerminalsStore.getState().focusedId !== terminalId) {
        agents.ringBell(terminalId);
      }
    });
    term.onTitleChange((title) => {
      useAgentsStore.getState().setTitleHint(terminalId, title);
    });
    term.onResize(({ cols, rows }) => {
      ipc.ptyResize(terminalId, cols, rows).catch(() => {});
    });

    // Ctrl/Cmd+C copies when there is a selection, Ctrl/Cmd+V pastes,
    // Ctrl/Cmd+F opens search. Everything else goes to the shell.
    term.attachCustomKeyEventHandler((e) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.type === "keydown") {
        if (e.key === "c" && term.hasSelection()) {
          navigator.clipboard.writeText(term.getSelection());
          return false;
        }
        if (e.key === "v") {
          // preventDefault, or Windows pastes twice. Returning false only stops
          // xterm's own key handling; the keydown's default action still runs,
          // and the browser paste event it produces reaches xterm's textarea
          // listener, which writes the text a second time. macOS never showed
          // this because the native Edit menu's accelerator (see menu.rs) eats
          // Cmd+V before the webview sees a keydown at all — this branch is
          // dead there, which is also why it went unnoticed.
          e.preventDefault();
          navigator.clipboard
            .readText()
            .then((text) => {
              // term.paste, not a raw write: it brackets the text when the
              // program asked for bracketed paste, so a multi-line paste is
              // offered for editing instead of each line executing as it
              // arrives, and it folds the clipboard's CRLFs down to CR.
              if (text) term.paste(text);
            })
            .catch(() => {});
          return false;
        }
        if (e.key === "f") {
          setSearchOpen(true);
          return false;
        }
      }
      return true;
    });

    // The shell's banner and first prompt arrive milliseconds after connecting.
    // A terminal opened into a workspace the user immediately switched away
    // from would otherwise announce its own greeting as new output.
    const quietUntil = Date.now() + 1200;

    // One parser per terminal, fed every chunk whether or not colouring is on:
    // it tracks the SGR and screen state, which would be stale by the time the
    // setting was turned back on if the stream had been skipped meanwhile.
    const highlight = createOutputHighlighter();

    const unlistenData = listen<string>(`pty:data:${terminalId}`, (event) => {
      const enabled = useSettingsStore.getState().terminal.highlightOutput;
      term.write(latin1ToBytes(highlight(atob(event.payload), enabled)));

      // "Was anyone looking when this arrived?" An unselected tab — and every
      // terminal of a workspace that is not the open one — is display:none and
      // measures 0×0, which is the same fact the resize handler below relies
      // on. A visible pane in an unfocused window counts as unwatched too: the
      // user is in another application.
      const el = hostRef.current;
      const watched =
        document.hasFocus() && !!el && el.clientWidth > 0 && el.clientHeight > 0;
      if (!watched && Date.now() > quietUntil) {
        useTerminalsStore.getState().noteActivity(terminalId);
      }
    });
    const unlistenStatus = listen<{ status: TerminalStatus; message?: string }>(
      `pty:status:${terminalId}`,
      (event) => {
        setStatus(terminalId, event.payload);
        // Only the shell itself failing to start is an error worth painting;
        // an ssh failure is ordinary shell output and needs no annotation.
        if (event.payload.status === "error" && event.payload.message) {
          term.writeln(`\r\n\x1b[31m✖ ${event.payload.message}\x1b[0m`);
        } else if (event.payload.status === "disconnected") {
          term.writeln("\r\n\x1b[90m— shell exited —\x1b[0m");
        }
      }
    );

    // An unselected tab is display:none, so its host measures 0×0. Fitting then
    // is worse than useless: FitAddon clamps to its 2×1 minimum, which resizes
    // the PTY and rebuilds the renderer at that size — and with WebGL the next
    // frame after the tab is shown again is that tiny grid scaled up, which is
    // the flash of huge letters. Ignore zero-size reports.
    let wasHidden = false;
    const observer = new ResizeObserver(() => {
      const el = hostRef.current;
      if (!el) return;
      if (el.clientWidth === 0 || el.clientHeight === 0) {
        wasHidden = true;
        return;
      }
      try {
        fit.fit();
      } catch {
        /* renderer not ready */
      }
      if (wasHidden) {
        wasHidden = false;
        // Coming back into view is the user looking, whether or not they then
        // click into the pane — which they never do for a log they are only
        // keeping an eye on.
        useTerminalsStore.getState().clearActivity(terminalId);
      }
    });
    observer.observe(hostRef.current);

    // Unconditional: a terminal with no connection is still a local shell.
    connect();

    return () => {
      cancelAnimationFrame(warmup);
      observer.disconnect();
      term.textarea?.removeEventListener("focus", onFocus);
      term.textarea?.removeEventListener("blur", onBlur);
      unlistenData.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      ipc.ptyClose(terminalId).catch(() => {});
      term.dispose();
      termRef.current = null;
      liveWebgls.delete(webglRef);
      webglRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminalId, connectionId]);

  // Click to move the shell cursor: only on the prompt line, only when the
  // remote app isn't capturing the mouse itself (vim, htop, …).
  useEffect(() => {
    const term = termRef.current;
    const host = hostRef.current;
    if (!term || !host) return;

    const onClick = (e: MouseEvent) => {
      if (e.button !== 0 || term.hasSelection()) return;
      if (term.modes.mouseTrackingMode !== "none") return;
      const dims = (term as any)._core?._renderService?.dimensions?.css?.cell;
      if (!dims?.width || !dims?.height) return;
      const rect = host.getBoundingClientRect();
      const col = Math.round((e.clientX - rect.left) / dims.width);
      const row = Math.floor((e.clientY - rect.top) / dims.height);
      const buf = term.buffer.active;
      // Only act on the row the cursor is already on.
      if (row !== buf.cursorY) return;
      const delta = col - buf.cursorX;
      if (delta === 0) return;
      const key = delta > 0 ? "\x1b[C" : "\x1b[D";
      ipc.ptyWrite(terminalId, key.repeat(Math.abs(delta))).catch(() => {});
    };

    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, [terminalId]);

  // Apply live settings/theme changes to the existing terminal.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    const theme = getTheme(themeId);
    term.options.cursorStyle = terminalSettings.cursorStyle;
    term.options.cursorBlink = terminalSettings.cursorBlink;
    term.options.theme = resolveTerminalTheme(theme, terminalSettings, accent);

    // The font options wait for the family to be usable at the new size, so
    // xterm never rasterises a frame in the fallback — that was the blink on
    // zoom. Resolved within the same tick when the font is already in, which
    // it is for every zoom step after the first at a given size.
    let stale = false;
    const stack = fontStack(terminalSettings.fontFamily);
    loadFonts(stack, terminalSettings.fontSize).then(() => {
      if (stale || termRef.current !== term) return;
      term.options.fontFamily = stack;
      term.options.fontSize = terminalSettings.fontSize;
      term.options.fontWeight = terminalSettings.fontWeight;
      term.options.fontWeightBold = terminalSettings.fontWeightBold;
      term.options.lineHeight = terminalSettings.lineHeight;
      // Same rule as the observer: never fit a hidden pane. The one it is
      // showing will be re-fitted by the observer as soon as it is displayed.
      const el = hostRef.current;
      if (el && el.clientWidth > 0 && el.clientHeight > 0) fitRef.current?.fit();
      // xterm refreshes itself on a font option change; the extra passes cover
      // what load() cannot promise for a local() face on every platform.
      refreshAtlasesSoon();
    });
    return () => {
      stale = true;
    };
  }, [terminalSettings, themeId, accent]);

  const hostBackground =
    resolveTerminalTheme(getTheme(themeId), terminalSettings, accent).background ??
    "var(--bg)";

  const doSearch = (dir: "next" | "prev") => {
    if (!query) return;
    if (dir === "next") searchRef.current?.findNext(query);
    else searchRef.current?.findPrevious(query);
  };

  return (
    <div
      className="terminal-host"
      style={{
        // No padding on the right: the scrollbar lives at the far edge of the
        // viewport, so any right padding would float it off the pane wall.
        padding: `${terminalSettings.padding}px 0 ${terminalSettings.padding}px ${terminalSettings.padding}px`,
        // Matches the terminal's own background so the padding and the
        // sub-row strip left over below the last line are invisible.
        background: hostBackground,
      }}
    >
      {searchOpen && (
        <div className="terminal-search">
          <input
            autoFocus
            placeholder="Search…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") doSearch(e.shiftKey ? "prev" : "next");
              if (e.key === "Escape") setSearchOpen(false);
            }}
          />
          <button className="btn" onClick={() => doSearch("prev")}>↑</button>
          <button className="btn" onClick={() => doSearch("next")}>↓</button>
          <button className="btn" onClick={() => setSearchOpen(false)}>✕</button>
        </div>
      )}
      <div ref={hostRef} style={{ width: "100%", height: "100%" }} />
    </div>
  );
}
