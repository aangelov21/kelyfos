import { create } from "zustand";
import { ipc } from "../lib/ipc";
import {
  AppTheme,
  applyUiTheme,
  defaultTerminalSettings,
  defaultUiSettings,
  getTheme,
  PALETTE_KEYS,
  setCustomThemes,
  TerminalSettings,
  UiSettings,
} from "../lib/themes";
import type { TagColorMap } from "../lib/tags";

interface SettingsState {
  ui: UiSettings;
  terminal: TerminalSettings;
  customThemes: AppTheme[];
  /** Tag name → hex. A tag exists as soon as some record wears it; this only
   *  records what colour it was given, so an unlisted tag is not an error. */
  tagColors: TagColorMap;
  loaded: boolean;
  load: () => Promise<void>;
  setUi: (patch: Partial<UiSettings>) => void;
  setTerminal: (patch: Partial<TerminalSettings>) => void;
  /** Colours are per tag, not per record: this repaints every chip wearing it. */
  setTagColor: (tag: string, color: string) => void;
  /** Persist a theme and switch to it, clearing the now-baked-in overrides. */
  saveCustomTheme: (theme: AppTheme) => void;
  deleteCustomTheme: (id: string) => void;
}

/**
 * xterm throws outright on a line height below 1 (OptionsService rejects it),
 * which would leave every terminal blank, so a stray value — typed past the
 * input's min, or left behind by an older build — is pulled back to 1.
 */
function sanitizeTerminal(terminal: TerminalSettings): TerminalSettings {
  const lineHeight = Number(terminal.lineHeight);
  const fixed =
    lineHeight >= 1
      ? terminal
      : { ...terminal, lineHeight: defaultTerminalSettings.lineHeight };

  // Only the 16 ANSI slots are editable. Anything else in a saved palette is a
  // leftover from a build that offered cursor and selection too, and it would
  // otherwise keep overriding every theme with no input left to clear it.
  const keys = Object.keys(fixed.palette);
  if (keys.every((k) => (PALETTE_KEYS as readonly string[]).includes(k))) return fixed;
  const palette: Record<string, string> = {};
  for (const key of PALETTE_KEYS) {
    if (fixed.palette[key] !== undefined) palette[key] = fixed.palette[key];
  }
  return { ...fixed, palette };
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  ui: defaultUiSettings,
  terminal: defaultTerminalSettings,
  customThemes: [],
  tagColors: {},
  loaded: false,

  load: async () => {
    const all = await ipc.settingsGetAll();
    const ui = { ...defaultUiSettings, ...(all["ui"] as object | undefined) };
    const terminal = sanitizeTerminal({
      ...defaultTerminalSettings,
      ...(all["terminal"] as object | undefined),
    });
    const customThemes = (all["customThemes"] as AppTheme[] | undefined) ?? [];
    const tagColors = (all["tagColors"] as TagColorMap | undefined) ?? {};
    // Register before resolving the theme — the saved id may be a custom one.
    setCustomThemes(customThemes);
    applyUiTheme(getTheme(ui.themeId), ui);
    set({ ui, terminal, customThemes, tagColors, loaded: true });
  },

  setUi: (patch) => {
    const ui = { ...get().ui, ...patch };
    set({ ui });
    applyUiTheme(getTheme(ui.themeId), ui);
    ipc.settingsSet("ui", ui);
  },

  setTerminal: (patch) => {
    const terminal = sanitizeTerminal({ ...get().terminal, ...patch });
    set({ terminal });
    ipc.settingsSet("terminal", terminal);
  },

  setTagColor: (tag, color) => {
    const tagColors = { ...get().tagColors, [tag]: color };
    set({ tagColors });
    ipc.settingsSet("tagColors", tagColors);
  },

  saveCustomTheme: (theme) => {
    const customThemes = [
      ...get().customThemes.filter((t) => t.id !== theme.id),
      theme,
    ];
    setCustomThemes(customThemes);
    // The overrides are baked into the saved theme, so keeping them would
    // double-apply and make the theme look un-switchable afterwards.
    const terminal = { ...get().terminal, palette: {} };
    delete terminal.foreground;
    delete terminal.background;
    // The accent is one of those overrides. Leaving it behind is what made the
    // colour look like it was never saved: the standing override won over every
    // theme's own accent, including this one's, so switching themes changed
    // nothing and the theme appeared to have dropped it.
    const ui = { ...get().ui, themeId: theme.id };
    delete ui.accent;

    set({ customThemes, terminal, ui });
    applyUiTheme(getTheme(ui.themeId), ui);
    ipc.settingsSet("customThemes", customThemes);
    ipc.settingsSet("terminal", terminal);
    ipc.settingsSet("ui", ui);
  },

  deleteCustomTheme: (id) => {
    const customThemes = get().customThemes.filter((t) => t.id !== id);
    setCustomThemes(customThemes);
    // Fall back to the default theme if the deleted one was selected.
    const ui =
      get().ui.themeId === id ? { ...get().ui, themeId: defaultUiSettings.themeId } : get().ui;

    set({ customThemes, ui });
    applyUiTheme(getTheme(ui.themeId), ui);
    ipc.settingsSet("customThemes", customThemes);
    ipc.settingsSet("ui", ui);
  },
}));
