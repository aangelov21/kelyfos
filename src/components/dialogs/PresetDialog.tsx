import { useEffect, useState } from "react";
import { ipc } from "../../lib/ipc";
import type { Preset, StartupTask } from "../../lib/types";
import { noAutoAssist } from "../../lib/inputProps";
import { useConnectionsStore } from "../../stores/connections";
import { emptyPreset, usePresetsStore } from "../../stores/presets";
import { Modal } from "../common/Modal";
import { Select } from "../common/Select";
import { TagsField } from "../common/TagsField";
import { StartupTasksEditor } from "./StartupTasksEditor";
import { VariablesEditor } from "./VariablesEditor";

interface Props {
  preset: Preset | null; // null = create new
  onClose: () => void;
}

/**
 * Editor for a reusable preset. Same four groups as the terminal dialog, since
 * a preset is the recipe a terminal is stamped from — a user who has seen one
 * already knows this one.
 */
export function PresetDialog({ preset, onClose }: Props) {
  const connections = useConnectionsStore((s) => s.connections);
  const savePreset = usePresetsStore((s) => s.save);

  const [draft, setDraft] = useState<Preset>(preset ?? emptyPreset());
  const [shells, setShells] = useState<string[]>([]);

  useEffect(() => {
    ipc.shellsList().then(setShells).catch(() => setShells([]));
  }, []);

  const set = (patch: Partial<Preset>) => setDraft((d) => ({ ...d, ...patch }));

  const save = async () => {
    if (!draft.name.trim()) return;
    await savePreset(draft);
    onClose();
  };

  return (
    <Modal
      title={preset ? "Edit preset" : "New preset"}
      onClose={onClose}
      width={540}
      footer={
        <>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={save} disabled={!draft.name.trim()}>
            {preset ? "Save" : "Create"}
          </button>
        </>
      }
    >
      <div className="form-section">
        <div className="form-section-title">Basic</div>
        <div className="form-row">
          <label>Name</label>
          <input
            {...noAutoAssist}
            autoFocus
            value={draft.name}
            placeholder="e.g. Deploy · staging"
            onChange={(e) => set({ name: e.target.value })}
          />
        </div>
        <div className="form-row">
          <label>Tags</label>
          <TagsField
            colored
            value={draft.tags}
            onChange={(tags) => set({ tags })}
            placeholder="Pick a tag, or type a new one"
          />
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          Connection
          <span className="hint">Where terminals from this preset open, and which shell they run.</span>
        </div>
        <div className="form-grid">
          <div className="form-row">
            <label>Connect to</label>
            <Select
              value={draft.connectionId ?? ""}
              onChange={(id) => set({ connectionId: id || null })}
              options={[
                { value: "", label: "This computer (local shell)" },
                ...connections.map((c) => ({
                  value: c.id,
                  label: `${c.name} (${c.username}@${c.host})`,
                })),
              ]}
            />
          </div>
          <div className="form-row">
            <label>Shell</label>
            <Select
              value={draft.shell}
              onChange={(shell) => set({ shell })}
              options={[
                { value: "", label: "Default from settings" },
                ...shells.map((s) => ({ value: s, label: s })),
              ]}
            />
          </div>
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          Startup
          <span className="hint">
            Commands sent automatically once the terminal is ready. Commands support {"${ENV}"}.
          </span>
        </div>
        <StartupTasksEditor
          tasks={draft.startupTasks}
          onChange={(startupTasks: StartupTask[]) => set({ startupTasks })}
        />
      </div>

      <div className="form-section">
        <div className="form-section-title">
          Variables
          <span className="hint">
            Copied onto every terminal opened from this preset, where they
            override the workspace and global ones.
          </span>
        </div>
        {preset ? (
          <VariablesEditor scope="preset" ownerId={preset.id} />
        ) : (
          <span className="hint">Available once the preset has been created.</span>
        )}
      </div>
    </Modal>
  );
}
