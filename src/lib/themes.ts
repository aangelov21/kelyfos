import type { ITheme } from "@xterm/xterm";
import { DEFAULT_STAT_PILLS } from "./statsPills";

export interface AppTheme {
  id: string;
  name: string;
  ui: {
    bg: string;
    bgPanel: string;
    bgElevated: string;
    border: string;
    text: string;
    textMuted: string;
    accent: string;
  };
  terminal: ITheme;
}

export const themes: AppTheme[] = [
  {
    id: "midnight",
    name: "Midnight",
    ui: {
      bg: "#0d1117",
      bgPanel: "#10161f",
      bgElevated: "#161d29",
      border: "#232b38",
      text: "#dbe4f0",
      textMuted: "#7d8b9f",
      accent: "#a78bfa",
    },
    terminal: {
      background: "#0d1117",
      foreground: "#fcf9ff",
      cursor: "#4f8cff",
      selectionBackground: "#2b3852",
      black: "#373f54",
      red: "#d6676c",
      green: "#44b486",
      yellow: "#e0a762",
      blue: "#5e8cf7",
      magenta: "#b69dff",
      cyan: "#3fafc0",
      white: "#edeaf2",
      brightBlack: "#7d8eb2",
      brightRed: "#ff8086",
      brightGreen: "#6ad3a8",
      brightYellow: "#ffc580",
      brightBlue: "#83a8ff",
      brightMagenta: "#c5b2ff",
      brightCyan: "#7ac8d9",
      brightWhite: "#fcf9ff",
    },
  },
  {
    id: "carbon",
    name: "Carbon",
    ui: {
      bg: "#121212",
      bgPanel: "#181818",
      bgElevated: "#202020",
      border: "#2c2c2c",
      text: "#e6e6e6",
      textMuted: "#8f8f8f",
      accent: "#a78bfa",
    },
    terminal: {
      background: "#121212",
      foreground: "#e6e6e6",
      cursor: "#3ddc84",
      selectionBackground: "#333f38",
      black: "#1e1e1e",
      red: "#f2777a",
      green: "#3ddc84",
      yellow: "#ffcc66",
      blue: "#6699cc",
      magenta: "#cc99cc",
      cyan: "#66cccc",
      white: "#e6e6e6",
      brightBlack: "#666666",
      brightRed: "#ff9a9d",
      brightGreen: "#6ef0a8",
      brightYellow: "#ffdd99",
      brightBlue: "#8cb8e6",
      brightMagenta: "#e0b3e0",
      brightCyan: "#8ce0e0",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    ui: {
      bg: "#1e1f29",
      bgPanel: "#232430",
      bgElevated: "#2a2c3a",
      border: "#3a3d4f",
      text: "#f8f8f2",
      textMuted: "#8b8fa5",
      accent: "#a78bfa",
    },
    terminal: {
      background: "#1e1f29",
      foreground: "#f8f8f2",
      cursor: "#bd93f9",
      selectionBackground: "#44475a",
      black: "#21222c",
      red: "#ff5555",
      green: "#50fa7b",
      yellow: "#f1fa8c",
      blue: "#bd93f9",
      magenta: "#ff79c6",
      cyan: "#8be9fd",
      white: "#f8f8f2",
      brightBlack: "#6272a4",
      brightRed: "#ff6e6e",
      brightGreen: "#69ff94",
      brightYellow: "#ffffa5",
      brightBlue: "#d6acff",
      brightMagenta: "#ff92df",
      brightCyan: "#a4ffff",
      brightWhite: "#ffffff",
    },
  },
];

/** The ITheme keys the palette editor exposes, in display order. */
export const PALETTE_KEYS = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

export type PaletteKey = (typeof PALETTE_KEYS)[number];

/** The editor exposes the 16 ANSI slots and nothing else. Cursor and selection
 *  used to sit beside them and were removed on purpose: neither is an ANSI
 *  colour, the cursor already follows the accent, and both belong to the theme.
 *  `sanitizeTerminal` drops any of them left in a saved palette. */
export const PALETTE_LABELS: Record<string, string> = {
  black: "Black",
  red: "Red",
  green: "Green",
  yellow: "Yellow",
  blue: "Blue",
  magenta: "Magenta",
  cyan: "Cyan",
  white: "White",
  brightBlack: "Bright Black",
  brightRed: "Bright Red",
  brightGreen: "Bright Green",
  brightYellow: "Bright Yellow",
  brightBlue: "Bright Blue",
  brightMagenta: "Bright Magenta",
  brightCyan: "Bright Cyan",
  brightWhite: "Bright White",
};

/**
 * User-saved themes, kept in a module registry so the many `getTheme(id)`
 * call sites keep working unchanged. The settings store owns the persisted
 * copy and pushes it here on load and on every save.
 */
let customThemes: AppTheme[] = [];

export function setCustomThemes(list: AppTheme[]) {
  customThemes = list;
}

export function allThemes(): AppTheme[] {
  return [...themes, ...customThemes];
}

export function getTheme(id: string): AppTheme {
  return allThemes().find((t) => t.id === id) ?? themes[0];
}

/** #rgb / #rrggbb with an alpha applied; any other notation passes through. */
function withAlpha(color: string, alpha: number): string {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim());
  if (!m) return color;
  const hex =
    m[1].length === 3
      ? m[1]
          .split("")
          .map((c) => c + c)
          .join("")
      : m[1];
  const n = parseInt(hex, 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** Theme palette with the user's per-slot overrides applied on top.
 *  The cursor follows the UI accent colour; it is not a palette slot. */
export function resolveTerminalTheme(
  theme: AppTheme,
  settings: TerminalSettings,
  accent?: string
): ITheme {
  // xterm defaults the scrollbar slider to the foreground at 20/40/50% opacity,
  // which reads as a grey bar in every theme. Tint it with the accent instead,
  // the same way the cursor follows it.
  // The effective accent: the user's override if there is one, otherwise the
  // theme's own. Both the cursor and the slider follow it, so a theme whose
  // accent is saved (rather than overridden) still tints them.
  const slider = accent || theme.ui.accent;
  return {
    ...theme.terminal,
    scrollbarSliderBackground: withAlpha(slider, 0.28),
    scrollbarSliderHoverBackground: withAlpha(slider, 0.45),
    scrollbarSliderActiveBackground: withAlpha(slider, 0.6),
    cursor: slider,
    ...settings.palette,
    ...(settings.foreground ? { foreground: settings.foreground } : {}),
    ...(settings.background ? { background: settings.background } : {}),
  };
}

export interface TerminalSettings {
  /** Shell to spawn for new terminals. Empty means the user's login shell. */
  shell: string;
  /** Default private-key path pre-filled into new SSH-key connections. */
  defaultKeyPath: string;
  /**
   * Prompt marking, which reports when a command starts and what it exited
   * with. On by default: it writes nothing the user owns — the shell is pointed
   * at generated startup files that source their own first — so the only reason
   * to refuse it is a shell setup it upsets.
   */
  shellIntegration: boolean;
  /**
   * Colour URLs and IP addresses that the program itself left uncoloured. Off
   * is the untouched stream; see `outputHighlight.ts` for what it does and does
   * not rewrite.
   */
  highlightOutput: boolean;
  fontFamily: string;
  fontSize: number;
  /** xterm FontWeight: "normal" | "bold" | 100–900. */
  fontWeight: number;
  /** Weight used for text the shell marks bold. */
  fontWeightBold: number;
  lineHeight: number;
  cursorStyle: "block" | "underline" | "bar";
  cursorBlink: boolean;
  padding: number;
  // Optional per-user overrides of the theme's terminal colors
  foreground?: string;
  background?: string;
  /** Per-slot ANSI/cursor/selection overrides, keyed by ITheme field name. */
  palette: Record<string, string>;
}

export const defaultTerminalSettings: TerminalSettings = {
  shell: "",
  defaultKeyPath: "",
  shellIntegration: true,
  highlightOutput: true,
  fontFamily: "Consolas, 'Cascadia Mono', Menlo, monospace",
  fontSize: 13,
  fontWeight: 400,
  fontWeightBold: 700,
  lineHeight: 1.25,
  cursorStyle: "block",
  cursorBlink: true,
  padding: 8,
  palette: {},
};

export interface UiSettings {
  themeId: string;
  /** Override on top of the active theme's accent; absent means "use the
   *  theme's own". Saving a custom theme bakes it in and clears it. */
  accent?: string;
  /** Connection folders the user has collapsed in the sidebar. */
  collapsedFolders: string[];
  /** Sidebar reduced to the workspace rail. Remembered, since it is a choice
   *  about how much screen the terminals get, not a transient view state. */
  sidebarCollapsed?: boolean;
  /** Show the strip of facts above each terminal. */
  statsBar: boolean;
  /** Which pills that strip shows, by key. Order comes from the catalogue, so
   *  this is a set, not a sequence. */
  statsPills: string[];
  /** Off by default: an update check is a network call the user didn't ask for. */
  checkUpdatesOnStartup?: boolean;
}

export const defaultUiSettings: UiSettings = {
  themeId: "midnight",
  // No accent override out of the box: every built-in theme already defines
  // #a78bfa, and a standing override would shadow the accent of any theme the
  // user later saves or switches to, which reads as "the theme didn't keep it".
  accent: undefined,
  collapsedFolders: [],
  sidebarCollapsed: false,
  statsBar: true,
  statsPills: DEFAULT_STAT_PILLS,
  checkUpdatesOnStartup: false,
};

export function applyUiTheme(theme: AppTheme, ui: UiSettings) {
  const root = document.documentElement;
  const c = theme.ui;
  root.style.setProperty("--bg", c.bg);
  root.style.setProperty("--bg-panel", c.bgPanel);
  root.style.setProperty("--bg-elevated", c.bgElevated);
  root.style.setProperty("--border", c.border);
  root.style.setProperty("--text", c.text);
  root.style.setProperty("--text-muted", c.textMuted);
  root.style.setProperty("--accent", ui.accent || c.accent);
}
