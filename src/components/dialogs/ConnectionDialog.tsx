import { useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import type { Connection } from "../../lib/types";
import { noAutoAssist } from "../../lib/inputProps";
import { emptyConnection, useConnectionsStore } from "../../stores/connections";
import { useSettingsStore } from "../../stores/settings";
import { Modal } from "../common/Modal";
import { Select } from "../common/Select";
import { TagsField } from "../common/TagsField";

interface Props {
  connection: Connection | null; // null = create new
  onClose: () => void;
}

/** Sentinel option value; no folder can be named this. */
const NEW_FOLDER = " new";

export function ConnectionDialog({ connection, onClose }: Props) {
  const save = useConnectionsStore((s) => s.save);
  const connections = useConnectionsStore((s) => s.connections);
  const defaultKeyPath = useSettingsStore((s) => s.terminal.defaultKeyPath);

  const [c, setC] = useState<Connection>(
    connection ?? { ...emptyConnection(), keyPath: defaultKeyPath || null }
  );
  const [error, setError] = useState("");
  const [creatingFolder, setCreatingFolder] = useState(false);

  const folders = useMemo(() => {
    const names = new Set(connections.map((x) => x.folder).filter(Boolean));
    // The connection's own folder may be the only one using it; keep it listed
    // so editing an unrelated field cannot silently move it out.
    if (c.folder) names.add(c.folder);
    return [...names].sort((a, b) => a.localeCompare(b));
  }, [connections, c.folder]);

  const patch = (p: Partial<Connection>) => setC({ ...c, ...p });

  const submit = async () => {
    if (!c.name.trim() || !c.host.trim() || !c.username.trim()) {
      setError("Name, host and username are required.");
      return;
    }
    try {
      await save(c);
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <Modal
      title={connection ? "Edit connection" : "New connection"}
      onClose={onClose}
      width={520}
      footer={
        <>
          {error && <span style={{ color: "var(--status-error)", marginRight: "auto" }}>{error}</span>}
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={submit}>Save</button>
        </>
      }
    >
      <div className="form-section">
        <div className="form-section-title">Basic</div>
        <div className="form-grid">
          <div className="form-row">
            <label>Name *</label>
            <input {...noAutoAssist} value={c.name} onChange={(e) => patch({ name: e.target.value })} autoFocus />
          </div>
          <div className="form-row">
            <label>Folder</label>
            {/* Picking from what exists prevents the near-miss folders ("Prod"
                vs "Production") that a free-text field invites; the last option
                switches to typing when a genuinely new one is needed. */}
            {creatingFolder ? (
              <div className="form-inline">
                <input
                  {...noAutoAssist}
                  autoFocus
                  style={{ flex: 1 }}
                  value={c.folder}
                  placeholder="e.g. Production"
                  onChange={(e) => patch({ folder: e.target.value })}
                />
                <button
                  className="btn"
                  title="Pick an existing folder instead"
                  onClick={() => {
                    setCreatingFolder(false);
                    patch({ folder: "" });
                  }}
                >
                  ✕
                </button>
              </div>
            ) : (
              <Select
                value={c.folder}
                onChange={(folder) => {
                  if (folder === NEW_FOLDER) {
                    setCreatingFolder(true);
                    patch({ folder: "" });
                  } else {
                    patch({ folder });
                  }
                }}
                options={[
                  { value: "", label: "No folder" },
                  ...folders.map((f) => ({ value: f, label: f })),
                  { value: NEW_FOLDER, label: "New folder…" },
                ]}
              />
            )}
          </div>
        </div>
        <div className="form-row">
          <label>Tags</label>
          <TagsField
            colored
            value={c.tags}
            onChange={(tags) => patch({ tags })}
            placeholder="Pick a tag, or type a new one"
          />
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">Connection</div>
        <div className="form-grid">
          <div className="form-row">
            <label>Host *</label>
            <input {...noAutoAssist} value={c.host} onChange={(e) => patch({ host: e.target.value })} />
          </div>
          <div className="form-row">
            <label>Port</label>
            <input
              {...noAutoAssist}
              type="number"
              value={c.port}
              onChange={(e) => patch({ port: Number(e.target.value) || 22 })}
            />
          </div>
        </div>
        <div className="form-row">
          <label>Username *</label>
          <input {...noAutoAssist} value={c.username} onChange={(e) => patch({ username: e.target.value })} />
        </div>

        <div className="form-row">
          <label>Authentication</label>
          <div className="form-inline">
            <label className="form-inline">
              <input
                type="radio"
                checked={c.authMethod === "password"}
                onChange={() => patch({ authMethod: "password" })}
              />
              Password
            </label>
            <label className="form-inline">
              <input
                type="radio"
                checked={c.authMethod === "key"}
                onChange={() => patch({ authMethod: "key" })}
              />
              SSH key
            </label>
          </div>
        </div>

        {c.authMethod === "password" && (
          <span className="hint">
            OpenSSH prompts for the password in the terminal.
          </span>
        )}

        {c.authMethod === "key" && (
          <div className="form-row">
            <label>Private key file</label>
            <input
              {...noAutoAssist}
              readOnly
              value={c.keyPath ?? ""}
              placeholder="Leave empty to use ~/.ssh/config"
              style={{ cursor: "pointer" }}
              onClick={async () => {
                const path = await openFileDialog({
                  multiple: false,
                  title: "Select private key",
                });
                if (typeof path === "string") patch({ keyPath: path });
              }}
            />
          </div>
        )}
      </div>
    </Modal>
  );
}
