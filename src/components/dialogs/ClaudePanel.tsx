import { useCallback, useEffect, useState } from "react";
import { ipc } from "../../lib/ipc";
import { STAT_PILLS } from "../../lib/statsPills";
import type { ClaudeHealth, ClaudeTestResult } from "../../lib/types";
import { useSettingsStore } from "../../stores/settings";

/** The hook transport differs enough per platform to be worth saying out loud. */
const isWindows = navigator.platform.toLowerCase().includes("win");

/**
 * Opt-in bridge to Claude Code, and the diagnostics for it.
 *
 * Connecting writes hooks into ~/.claude/settings.json — a file this app does
 * not own — so it happens on a button press, never on a launch, and the screen
 * says what it will do before it does it. Everything here is inert on a machine
 * without Claude Code; the terminal behaves identically either way.
 */
export function ClaudePanel() {
  const [health, setHealth] = useState<ClaudeHealth | null>(null);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<ClaudeTestResult | null>(null);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    try {
      setHealth(await ipc.claudeHealth());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Sends a real hook through the real script, rather than re-reading the same
  // four facts and appearing to do nothing.
  const doTest = async () => {
    setTesting(true);
    setTest(null);
    try {
      setTest(await ipc.claudeTest());
    } catch (e) {
      setTest({ ok: false, message: String(e) });
    }
    await refresh();
    setTesting(false);
  };

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
    await refresh();
    setBusy(false);
  };

  // Rendered in every branch, not only the connected one: the strip also
  // carries the git branch, which is worth having on a machine that has never
  // run Claude, so its settings must not be gated on Claude being present.
  const statusBarSettings = <StatusBarSettings />;

  if (!health) {
    return (
      <>
        <span className="hint">Checking…</span>
        {statusBarSettings}
      </>
    );
  }

  if (!health.installed) {
    return (
      <>
        <div className="form-row">
          <span className="hint">
            Claude Code was not found on this computer. Kelyfos works exactly the
            same without it — this section only adds status for terminals that
            are running it.
          </span>
        </div>
        {statusBarSettings}
      </>
    );
  }

  return (
    <>
      <div className="form-row">
        <label>Claude Code</label>
        <span className="hint">
          Detected{health.version ? ` — ${health.version}` : ""}. Terminals
          running Claude are marked automatically; connecting adds the working
          and waiting-for-you states on top.
        </span>
      </div>

      {/* Claude follows this app's terminal palette only when it draws with
          ANSI colors. Its stock dark-ansi theme does, but overloads ANSI white
          (dim text and block backgrounds share the slot), so Kelyfos offers a
          custom theme file that keeps text on the 16 slots and moves the
          backgrounds to fixed greys. Installing writes one file the user was
          told about; *selecting* it stays theirs — settings.json's theme key
          is never written, /theme inside Claude is one command away. */}
      <ClaudeThemeRow health={health} busy={busy} run={run} />

      {!health.connected && (
        <div className="consent-panel">
          <strong>Connect to Claude</strong>
          {/* Stated before the write, not after: this touches a file the user
              owns, and a reasonable person assumes the worst if not told. */}
          <ul className="consent-list">
            <li>
              Adds Kelyfos hooks to <code>{health.settingsPath ?? "~/.claude/settings.json"}</code>.
              Your existing settings and any other tool's hooks are left alone,
              and the file is backed up first.
            </li>
            <li>
              Installs a small script at <code>{health.shimPath ?? "the Kelyfos data folder"}</code>{" "}
              that forwards events to this app.
            </li>
            <li>
              Sends Kelyfos the event name, session id and working directory —
              never the contents of your conversations.
            </li>
            {isWindows && (
              // Consent-relevant, not trivia: this is a cost the user pays on
              // every tool call for the rest of the session, and they are
              // agreeing to it now. macOS runs a /bin/sh script instead, which
              // starts in single-digit milliseconds and is not worth a line.
              <li>
                On Windows each event runs a short PowerShell script, which adds
                roughly 0.2s to every one of Claude's tool calls. Disconnect if
                you would rather have the speed than the working and
                waiting-for-you states.
              </li>
            )}
            <li>Disconnect undoes all of it, including the script.</li>
          </ul>
          <button
            className="btn primary"
            disabled={busy}
            onClick={() => run(ipc.claudeConnect)}
          >
            {busy ? "Connecting…" : "Connect to Claude"}
          </button>
          {health.partial && (
            <span className="hint">
              Some Kelyfos hooks are present but not all — usually a hand-edited
              settings.json. Connecting again repairs it.
            </span>
          )}
        </div>
      )}

      {health.connected && (
        <div className="form-row">
          <label>Connection</label>
          {/* Every link reported separately: a break anywhere in the chain
              otherwise looks exactly like "Claude is idle", and neither the
              user nor a bug report can tell which hop failed. */}
          <div className="health-list">
            <HealthRow ok label="Claude Code installed" />
            <HealthRow ok={health.connected} label="Hooks installed" />
            <HealthRow ok={health.shimInstalled} label="Helper script present" />
            <HealthRow ok={health.socketListening} label="Kelyfos listening" />
            {/* The ticks only say each piece exists. Whether Claude has ever
                actually reported anything is the part that tells you the
                bridge works, and it is invisible without this line. */}
            <HealthRow
              ok={health.lastEventAt !== null}
              label={
                health.lastEventAt
                  ? `Last event ${new Date(health.lastEventAt).toLocaleTimeString()}`
                  : "No events received yet — start Claude in a terminal"
              }
            />
          </div>
          <div className="form-inline" style={{ marginTop: 8 }}>
            <button className="btn" disabled={busy} onClick={doTest}>
              {testing ? "Testing…" : "Test connection"}
            </button>
            <button
              className="btn"
              disabled={busy}
              onClick={() => run(ipc.claudeDisconnect)}
            >
              Disconnect
            </button>
          </div>
          {test && (
            <span
              className="hint"
              style={{
                color: test.ok ? "var(--status-connected)" : "var(--status-error)",
              }}
            >
              {test.ok ? "✓ " : "✕ "}
              {test.message}
            </span>
          )}
        </div>
      )}

      {error && <span className="hint" style={{ color: "var(--status-error)" }}>{error}</span>}

      {statusBarSettings}
    </>
  );
}

/** What Claude selects the installed file as: "custom:" + the file stem. */
const KELYFOS_THEME = "custom:kelyfos-theme";

function ClaudeThemeRow({
  health,
  busy,
  run,
}: {
  health: ClaudeHealth;
  busy: boolean;
  run: (action: () => Promise<void>) => Promise<void>;
}) {
  const selected = health.theme === KELYFOS_THEME;

  return (
    <div className="form-row">
      <label>Theme</label>
      {health.themeInstalled && selected ? (
        <span className="hint" style={{ color: "var(--status-connected)" }}>
          ✓ Claude is using the Kelyfos theme, so its colors follow this app's
          terminal palette.
        </span>
      ) : health.themeInstalled ? (
        <span className="hint" style={{ color: "var(--status-pending)" }}>
          The Kelyfos theme is installed but Claude is set to "
          {health.theme ?? "default"}". Run <code>/theme</code> inside Claude
          and pick <em>kelyfos-theme</em> to make it follow this app's terminal
          palette.
        </span>
      ) : (
        <>
          <span className="hint">
            Claude is set to "{health.theme ?? "default"}", which does not
            follow this app's terminal palette. Installing the Kelyfos theme
            writes one file, <code>~/.claude/themes/kelyfos-theme.json</code> —
            nothing else is touched, and Claude's theme setting stays yours.
            {/* Even when the file is merely stale rather than absent, the same
                button is the fix: installing overwrites with the current one. */}
          </span>
          <div className="form-inline" style={{ marginTop: 6 }}>
            <button
              className="btn"
              disabled={busy}
              onClick={() => run(ipc.claudeThemeInstall)}
            >
              Install Kelyfos theme
            </button>
          </div>
        </>
      )}
      {health.themeInstalled && (
        <div className="form-inline" style={{ marginTop: 6 }}>
          <button
            className="btn"
            disabled={busy}
            onClick={() => run(ipc.claudeThemeRemove)}
          >
            Remove theme file
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * The strip of pills above each terminal: whether to show it, and which of the
 * pills it may show. The list is the pill catalogue itself, so a pill can never
 * be renderable but absent from this picker.
 */
function StatusBarSettings() {
  const statsBar = useSettingsStore((s) => s.ui.statsBar);
  const statsPills = useSettingsStore((s) => s.ui.statsPills);
  const setUi = useSettingsStore((s) => s.setUi);

  const toggle = (key: string) =>
    setUi({
      statsPills: statsPills.includes(key)
        ? statsPills.filter((k) => k !== key)
        : [...statsPills, key],
    });

  return (
    <div className="form-row">
      <label>Terminal status bar</label>
      <div className="switch-row">
        <button
          role="switch"
          aria-checked={statsBar}
          className={`switch${statsBar ? " on" : ""}`}
          onClick={() => setUi({ statsBar: !statsBar })}
        >
          <span className="switch-knob" />
        </button>
        <span>Show a strip of facts above each terminal</span>
      </div>

      {statsBar && (
        <>
          <div className="form-inline" style={{ margin: "8px 0 6px" }}>
            <span className="hint" style={{ marginRight: "auto" }}>
              {statsPills.length} of {STAT_PILLS.length} shown. A pill only
              appears when it has something to report.
            </span>
            <button
              className="btn"
              onClick={() => setUi({ statsPills: STAT_PILLS.map((p) => p.key) })}
            >
              All
            </button>
            <button className="btn" onClick={() => setUi({ statsPills: [] })}>
              None
            </button>
          </div>
          <div className="pill-options">
            {STAT_PILLS.map((p) => (
              <label key={p.key} className="pill-option" title={p.description}>
                <input
                  type="checkbox"
                  checked={statsPills.includes(p.key)}
                  onChange={() => toggle(p.key)}
                />
                <span>
                  <span className="pill-option-label">{p.label}</span>
                  <span className="pill-option-desc">{p.description}</span>
                </span>
              </label>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function HealthRow({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="health-row">
      <span className={`health-mark${ok ? " ok" : ""}`}>{ok ? "✓" : "✕"}</span>
      <span>{label}</span>
    </div>
  );
}
