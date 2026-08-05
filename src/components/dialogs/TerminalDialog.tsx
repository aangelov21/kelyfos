import { useEffect, useState } from "react";
import type { StartupTask, TerminalDef } from "../../lib/types";
import { ipc } from "../../lib/ipc";
import { noAutoAssist } from "../../lib/inputProps";
import { useConnectionsStore } from "../../stores/connections";
import { useTerminalsStore } from "../../stores/terminals";
import { Modal } from "../common/Modal";
import { Select } from "../common/Select";
import { StartupTasksEditor } from "./StartupTasksEditor";
import { VariablesEditor } from "./VariablesEditor";

interface Props {
  workspaceId: string;
  terminal: TerminalDef | null; // null = create new
  onClose: () => void;
  onSaved: (t: TerminalDef, isNew: boolean) => void;
}

export function TerminalDialog({ workspaceId, terminal, onClose, onSaved }: Props) {
  const connections = useConnectionsStore((s) => s.connections);
  const saveTerminal = useTerminalsStore((s) => s.save);

  const [title, setTitle] = useState(terminal?.title ?? "");
  // "" is the local shell, and a local terminal stores connectionId as null —
  // so no fallback here: anything else would show a saved local terminal as
  // being connected to whichever connection happened to come first.
  const [connectionId, setConnectionId] = useState(terminal?.connectionId ?? "");
  const [tasks, setTasks] = useState<StartupTask[]>(terminal?.startupTasks ?? []);
  const [shell, setShell] = useState(terminal?.shell ?? "");
  const [shells, setShells] = useState<string[]>([]);

  useEffect(() => {
    ipc.shellsList().then(setShells).catch(() => setShells([]));
  }, []);

  const save = async () => {
    const saved = await saveTerminal({
      id: terminal?.id ?? "",
      workspaceId,
      connectionId: connectionId || null,
      title,
      startupTasks: tasks,
      sortOrder: terminal?.sortOrder ?? 0,
      shell,
    });
    onSaved(saved, !terminal);
    onClose();
  };

  return (
    <Modal
      title={terminal ? "Terminal settings" : "New terminal"}
      onClose={onClose}
      width={540}
      footer={
        <>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={save}>
            {terminal ? "Save" : "Create"}
          </button>
        </>
      }
    >
      <div className="form-section">
        <div className="form-section-title">
          Basic
        </div>
        <div className="form-row">
          <label>Title</label>
          <input
            {...noAutoAssist}
            value={title}
            placeholder="Leave empty to use the connection name"
            onChange={(e) => setTitle(e.target.value)}
          />
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          Connection
          <span className="hint">Where this terminal opens, and which shell it runs.</span>
        </div>
        <div className="form-grid">
          <div className="form-row">
            <label>Connect to</label>
            <Select
              value={connectionId}
              onChange={setConnectionId}
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
              value={shell}
              onChange={setShell}
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
          <span className="hint">Commands sent automatically once the terminal is ready. Commands support {"${ENV}"}.</span>
        </div>
        <StartupTasksEditor tasks={tasks} onChange={setTasks} />
      </div>

      <div className="form-section">
        <div className="form-section-title">
          Variables
          <span className="hint">
            Used in the title and startup commands. These override the workspace
            and global ones.
          </span>
        </div>
        {terminal ? (
          <VariablesEditor scope="terminal" ownerId={terminal.id} />
        ) : (
          <span className="hint">Available once the terminal has been created.</span>
        )}
      </div>
    </Modal>
  );
}
