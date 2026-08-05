import { useEffect, useState } from "react";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { ipc } from "../../lib/ipc";
import { exportAll, importAll } from "../../lib/backup";
import { noAutoAssist } from "../../lib/inputProps";
import {
  backupAndInstall,
  checkForUpdate,
  currentVersion,
  formatBytes,
  restartToInstall,
  type CheckResult,
  type InstallProgress,
} from "../../lib/updater";
import {
  getTheme,
  PALETTE_KEYS,
  PALETTE_LABELS,
  themes,
  type AppTheme,
} from "../../lib/themes";
import { useConnectionsStore } from "../../stores/connections";
import { usePresetsStore } from "../../stores/presets";
import { useSettingsStore } from "../../stores/settings";
import { useWorkspacesStore } from "../../stores/workspaces";
import { ColorField } from "../common/ColorField";
import { Modal } from "../common/Modal";
import { Select } from "../common/Select";
import { ClaudePanel } from "./ClaudePanel";
import { VariablesEditor } from "./VariablesEditor";

interface Props {
  onClose: () => void;
  /** Which accordion pane starts open; the startup update check aims at "advanced". */
  initialSection?: SectionId;
}

type SectionId = "terminal" | "claude" | "variables" | "appearance" | "advanced";

/**
 * Palette fields in display order, interleaved so that a two-column row-flow
 * grid puts each normal color beside its bright counterpart (black next to
 * bright black, and so on).
 */
const PALETTE_ROWS: string[] = PALETTE_KEYS.slice(0, 8).flatMap((key, i) => [
  key,
  PALETTE_KEYS[i + 8],
]);

/** A font renders the nearest weight it actually ships, so offering the full
 *  CSS range is harmless even for families with only one or two cuts. */
const WEIGHTS = [
  { value: 100, label: "100 · Thin" },
  { value: 200, label: "200 · Extra Light" },
  { value: 300, label: "300 · Light" },
  { value: 400, label: "400 · Normal" },
  { value: 500, label: "500 · Medium" },
  { value: 600, label: "600 · Semi Bold" },
  { value: 700, label: "700 · Bold" },
  { value: 800, label: "800 · Extra Bold" },
  { value: 900, label: "900 · Black" },
];

/** One accordion pane. Only one is open at a time; the closed→open height
 *  transition rides `grid-template-rows: 0fr → 1fr`, which animates to the
 *  content's natural height with no measuring and no end-of-animation jump. */
function Section({
  id,
  title,
  open,
  onOpen,
  children,
}: {
  id: SectionId;
  title: string;
  open: boolean;
  onOpen: (id: SectionId) => void;
  children: React.ReactNode;
}) {
  return (
    <div className="accordion">
      <button
        className={`accordion-head${open ? " open" : ""}`}
        aria-expanded={open}
        onClick={() => onOpen(id)}
      >
        <span className={`chevron ${open ? "open" : ""}`} aria-hidden="true" />
        {title}
      </button>
      <div className={`accordion-panel${open ? " open" : ""}`}>
        <div className="accordion-inner">
          <div className="accordion-body">{children}</div>
        </div>
      </div>
    </div>
  );
}

export function SettingsDialog({ onClose, initialSection = "terminal" }: Props) {
  const ui = useSettingsStore((s) => s.ui);
  const terminal = useSettingsStore((s) => s.terminal);
  const setUi = useSettingsStore((s) => s.setUi);
  const setTerminal = useSettingsStore((s) => s.setTerminal);
  const [shells, setShells] = useState<string[]>([]);
  const [fonts, setFonts] = useState<{ monospace: string[]; all: string[] }>({
    monospace: [],
    all: [],
  });
  const [open, setOpen] = useState<SectionId>(initialSection);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [themeName, setThemeName] = useState("");
  const [ioMsg, setIoMsg] = useState("");
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);
  const [check, setCheck] = useState<CheckResult | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [installState, setInstallState] = useState<"idle" | "working" | "done">("idle");
  const [updateError, setUpdateError] = useState("");
  const customThemes = useSettingsStore((s) => s.customThemes);
  const saveCustomTheme = useSettingsStore((s) => s.saveCustomTheme);
  const deleteCustomTheme = useSettingsStore((s) => s.deleteCustomTheme);

  useEffect(() => {
    ipc.shellsList().then(setShells).catch(() => setShells([]));
    ipc.fontsList().then(setFonts).catch(() => setFonts({ monospace: [], all: [] }));
    currentVersion().then(setVersion).catch(() => setVersion(""));
  }, []);

  const doCheck = async () => {
    setChecking(true);
    setUpdateError("");
    setProgress(null);
    setInstallState("idle");
    setCheck(await checkForUpdate());
    setChecking(false);
  };

  const doInstall = async () => {
    if (check?.kind !== "available") return;
    setInstallState("working");
    setUpdateError("");
    try {
      await backupAndInstall(check.update, setProgress);
      setInstallState("done");
    } catch (e) {
      // Covers the backup failing too, in which case nothing was installed.
      setUpdateError(String(e));
      setInstallState("idle");
    }
  };

  const themeTerminal = getTheme(ui.themeId).terminal;
  // Keep a value the scan didn't return (a legacy CSS stack, or a font that has
  // since been uninstalled) selectable, so opening Settings never silently
  // rewrites the user's choice.
  // Monospace only: a terminal is a character grid, and a proportional family
  // throws the columns off it.
  const fontList = fonts.monospace.includes(terminal.fontFamily)
    ? fonts.monospace
    : [terminal.fontFamily, ...fonts.monospace];

  const base = getTheme(ui.themeId);
  const overrideCount =
    Object.keys(terminal.palette).length +
    (terminal.foreground ? 1 : 0) +
    (terminal.background ? 1 : 0);
  const editingCustom = customThemes.some((t) => t.id === ui.themeId);

  const setSlot = (key: string, value: string | undefined) => {
    const palette = { ...terminal.palette };
    if (value === undefined) delete palette[key];
    else palette[key] = value;
    setTerminal({ palette });
  };

  const browseDefaultKey = async () => {
    const path = await openFileDialog({
      multiple: false,
      title: "Select default private key",
    });
    if (typeof path === "string") setTerminal({ defaultKeyPath: path });
  };

  const doExport = async () => {
    try {
      const json = await exportAll();
      const path = await saveFileDialog({
        defaultPath: "kelyfos-backup.json",
        filters: [{ name: "Backup", extensions: ["json"] }],
      });
      if (path) {
        await writeTextFile(path, json);
        setIoMsg("Exported.");
      }
    } catch (e) {
      setIoMsg(String(e));
    }
  };

  const doImport = async () => {
    const path = await openFileDialog({
      multiple: false,
      filters: [{ name: "Backup", extensions: ["json"] }],
    });
    if (typeof path !== "string") return;
    try {
      await importAll(await readTextFile(path));
      await Promise.all([
        useSettingsStore.getState().load(),
        useConnectionsStore.getState().load(),
        // Presets were left out, so imported ones only appeared after a restart.
        usePresetsStore.getState().load(),
        useWorkspacesStore.getState().load(),
      ]);
      setIoMsg("Imported.");
    } catch (e) {
      setIoMsg(String(e));
    }
  };

  /** Bake the current theme plus every override into a standalone theme. */
  const saveAsTheme = () => {
    const name = themeName.trim();
    if (!name) return;
    const theme: AppTheme = {
      id: `custom-${crypto.randomUUID()}`,
      name,
      ui: { ...base.ui, ...(ui.accent ? { accent: ui.accent } : {}) },
      terminal: {
        ...base.terminal,
        ...terminal.palette,
        ...(terminal.foreground ? { foreground: terminal.foreground } : {}),
        ...(terminal.background ? { background: terminal.background } : {}),
      },
    };
    saveCustomTheme(theme);
    setThemeName("");
  };

  return (
    <Modal
      title="Settings"
      onClose={onClose}
      width={560}
      footer={<button className="btn primary" onClick={onClose}>Done</button>}
    >
      <Section id="terminal" title="Terminal" open={open === "terminal"} onOpen={setOpen}>
        <div className="form-row">
          <label>Default shell</label>
          <Select
            value={terminal.shell}
            onChange={(shell) => setTerminal({ shell })}
            options={[
              { value: "", label: "— login shell ($SHELL) —" },
              ...shells.map((s) => ({ value: s, label: s })),
            ]}
          />
        </div>
        <div className="form-row">
          <label>Default private SSH key</label>
          <div className="form-inline">
            <input
              {...noAutoAssist}
              readOnly
              value={terminal.defaultKeyPath}
              placeholder="No default — new connections start empty"
              style={{ flex: 1, cursor: "pointer" }}
              title={terminal.defaultKeyPath || undefined}
              onClick={browseDefaultKey}
            />
            <button className="btn" onClick={browseDefaultKey}>Browse…</button>
            {terminal.defaultKeyPath && (
              <button className="btn" onClick={() => setTerminal({ defaultKeyPath: "" })}>
                Clear
              </button>
            )}
          </div>
          <span className="hint">
            Pre-filled as the key file when you create a new SSH-key connection.
          </span>
        </div>
        <div className="form-row">
          <label>Shell integration</label>
          <div className="switch-row">
            <button
              role="switch"
              aria-checked={terminal.shellIntegration}
              className={`switch${terminal.shellIntegration ? " on" : ""}`}
              onClick={() =>
                setTerminal({ shellIntegration: !terminal.shellIntegration })
              }
            >
              <span className="switch-knob" />
            </button>
            <span>Track when commands start and whether they fail</span>
          </div>
          <span className="hint">
            Lets a tab show a real busy indicator and flag a command that exited
            with an error. Your own startup files are loaded first and are never
            modified. zsh and bash only; takes effect in new terminals.
          </span>
        </div>
        <div className="form-row">
          <label>Highlight output</label>
          <div className="switch-row">
            <button
              role="switch"
              aria-checked={terminal.highlightOutput}
              className={`switch${terminal.highlightOutput ? " on" : ""}`}
              onClick={() =>
                setTerminal({ highlightOutput: !terminal.highlightOutput })
              }
            >
              <span className="switch-knob" />
            </button>
            <span>Color URLs and IP addresses in plain output</span>
          </div>
          <span className="hint">
            Only text a program left uncolored is touched, and never inside a
            full-screen program such as vim, htop or tmux. Applies immediately,
            to output that arrives from now on.
          </span>
        </div>
      </Section>

      <Section id="claude" title="Claude Code" open={open === "claude"} onOpen={setOpen}>
        <ClaudePanel />
      </Section>

      <Section
        id="variables"
        title="Global variables"
        open={open === "variables"}
        onOpen={setOpen}
      >
        <VariablesEditor scope="global" ownerId={null} />
      </Section>

      <Section
        id="appearance"
        title="Appearance"
        open={open === "appearance"}
        onOpen={setOpen}
      >
        <div className="form-grid">
          <div className="form-row">
            <label>Theme</label>
            <div className="form-inline">
              <Select
                style={{ flex: 1 }}
                value={ui.themeId}
                onChange={(themeId) => setUi({ themeId })}
                options={[
                  ...themes.map((t) => ({ value: t.id, label: t.name })),
                  ...customThemes.map((t) => ({
                    value: t.id,
                    label: t.name,
                    group: "Saved",
                  })),
                ]}
              />
              {editingCustom && (
                <button
                  className="btn icon danger"
                  title="Delete this saved theme"
                  onClick={() => deleteCustomTheme(ui.themeId)}
                >
                  🗑
                </button>
              )}
            </div>
          </div>
          <div className="form-row">
            <label>Accent color</label>
            <ColorField
              value={ui.accent}
              fallback={base.ui.accent}
              onChange={(accent) => setUi({ accent })}
            />
          </div>
          {/* Next to the theme they override, not down with the font controls:
              all three answer "what colour is this". */}
          <div className="form-row">
            <label>Foreground override</label>
            <ColorField
              value={terminal.foreground}
              fallback={themeTerminal.foreground ?? "#ffffff"}
              onChange={(foreground) => setTerminal({ foreground })}
            />
          </div>
          <div className="form-row">
            <label>Background override</label>
            <ColorField
              value={terminal.background}
              fallback={themeTerminal.background ?? "#000000"}
              onChange={(background) => setTerminal({ background })}
            />
          </div>
        </div>

        <div className="form-grid">
          <div className="form-row">
            <label>Font family</label>
            <Select
              value={terminal.fontFamily}
              title={terminal.fontFamily}
              onChange={(fontFamily) => setTerminal({ fontFamily })}
              options={fontList.map((f) => ({ value: f, label: f, font: f }))}
            />
          </div>
          <div className="form-row">
            <label>Font size</label>
            <input
              {...noAutoAssist}
              type="number"
              min={8}
              max={32}
              value={terminal.fontSize}
              onChange={(e) => setTerminal({ fontSize: Number(e.target.value) || 13 })}
            />
          </div>
          <div className="form-row">
            <label>Font weight</label>
            <Select
              value={String(terminal.fontWeight)}
              onChange={(v) => setTerminal({ fontWeight: Number(v) })}
              options={WEIGHTS.map((w) => ({ value: String(w.value), label: w.label }))}
            />
          </div>
          <div className="form-row">
            <label>Bold weight</label>
            <Select
              value={String(terminal.fontWeightBold)}
              onChange={(v) => setTerminal({ fontWeightBold: Number(v) })}
              options={WEIGHTS.map((w) => ({ value: String(w.value), label: w.label }))}
            />
          </div>
          <div className="form-row">
            <label>Line height</label>
            <input
              {...noAutoAssist}
              type="number"
              step={0.05}
              min={1}
              max={2}
              value={terminal.lineHeight}
              onChange={(e) => setTerminal({ lineHeight: Number(e.target.value) || 1.25 })}
            />
          </div>
          <div className="form-row">
            <label>Padding (px)</label>
            <input
              {...noAutoAssist}
              type="number"
              min={0}
              max={40}
              value={terminal.padding}
              onChange={(e) => setTerminal({ padding: Number(e.target.value) || 0 })}
            />
          </div>
          <div className="form-row">
            <label>Cursor</label>
            <Select
              value={terminal.cursorStyle}
              onChange={(v) =>
                setTerminal({ cursorStyle: v as "block" | "underline" | "bar" })
              }
              options={[
                { value: "block", label: "Block" },
                { value: "underline", label: "Underline" },
                { value: "bar", label: "Bar" },
              ]}
            />
          </div>
          <div className="form-row">
            <label>Cursor blink</label>
            <Select
              value={terminal.cursorBlink ? "on" : "off"}
              onChange={(v) => setTerminal({ cursorBlink: v === "on" })}
              options={[
                { value: "on", label: "On" },
                { value: "off", label: "Off" },
              ]}
            />
          </div>
        </div>

        <button
          className="settings-section collapsible"
          aria-expanded={paletteOpen}
          onClick={() => setPaletteOpen((v) => !v)}
        >
          <span className={`chevron ${paletteOpen ? "open" : ""}`} aria-hidden="true" />
          Palette
          {overrideCount > 0 && <span className="badge">{overrideCount} changed</span>}
        </button>

        {paletteOpen && (
          <>
            <div className="palette-grid">
              {PALETTE_ROWS.map((key) => (
                <div className="form-row" key={key}>
                  <label>{PALETTE_LABELS[key]}</label>
                  <ColorField
                    value={terminal.palette[key]}
                    fallback={(base.terminal as Record<string, string>)[key] ?? "#000000"}
                    onChange={(v) => setSlot(key, v)}
                  />
                </div>
              ))}
            </div>

            <div className="form-row" style={{ marginTop: 14 }}>
              <label>Save current colors as a theme</label>
              <div className="form-inline">
                <input
                  {...noAutoAssist}
                  value={themeName}
                  placeholder="Theme name"
                  style={{ flex: 1 }}
                  onChange={(e) => setThemeName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && saveAsTheme()}
                />
                <button
                  className="btn primary"
                  disabled={!themeName.trim()}
                  onClick={saveAsTheme}
                >
                  Save theme
                </button>
              </div>
              <span className="hint">
                Bakes the current theme plus every override into a new entry in the
                theme list, then clears the overrides.
              </span>
            </div>
          </>
        )}
      </Section>

      <Section id="advanced" title="Advanced" open={open === "advanced"} onOpen={setOpen}>
        <div className="form-row">
          <label>Import / export</label>
          <div className="form-inline">
            <button className="btn" onClick={doExport}>Export settings…</button>
            <button className="btn" onClick={doImport}>Import settings…</button>
            {ioMsg && <span className="hint" style={{ marginLeft: 4 }}>{ioMsg}</span>}
          </div>
        </div>

        <div className="form-row" style={{ marginTop: 14 }}>
          <label>Updates</label>
          <div className="form-inline">
            <span className="hint" style={{ marginRight: 4 }}>
              Version {version || "…"}
            </span>
            <button className="btn" onClick={doCheck} disabled={checking || installState === "working"}>
              {checking ? "Checking…" : "Check for updates"}
            </button>
          </div>

          {check?.kind === "up-to-date" && (
            <span className="hint">You are on the latest version.</span>
          )}
          {check?.kind === "failed" && (
            <span className="hint">Could not check for updates: {check.error}</span>
          )}
          {check?.kind === "available" && (
            <div className="update-panel">
              <strong>Version {check.update.version} is available.</strong>
              {check.update.body && <p className="hint">{check.update.body}</p>}

              {installState === "idle" && (
                <button className="btn primary" onClick={doInstall}>
                  Install
                </button>
              )}
              {installState === "working" && (
                <span className="hint">
                  {progress
                    ? progress.total
                      ? `Downloading… ${formatBytes(progress.downloaded)} of ${formatBytes(progress.total)}`
                      : `Downloading… ${formatBytes(progress.downloaded)}`
                    : "Backing up database…"}
                </span>
              )}
              {installState === "done" && (
                <div className="form-inline">
                  <span className="hint">Installed.</span>
                  <button className="btn primary" onClick={restartToInstall}>
                    Restart to install
                  </button>
                </div>
              )}
              {updateError && <span className="hint">Update stopped: {updateError}</span>}
            </div>
          )}

          <div className="switch-row" style={{ marginTop: 10 }}>
            <button
              role="switch"
              aria-checked={ui.checkUpdatesOnStartup ?? false}
              className={`switch${ui.checkUpdatesOnStartup ? " on" : ""}`}
              onClick={() => setUi({ checkUpdatesOnStartup: !ui.checkUpdatesOnStartup })}
            >
              <span className="switch-knob" />
            </button>
            <span>Check for updates on startup</span>
          </div>
        </div>
      </Section>
    </Modal>
  );
}
