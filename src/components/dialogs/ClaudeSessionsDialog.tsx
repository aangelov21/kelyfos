import { useEffect, useState } from "react";
import { ipc } from "../../lib/ipc";
import type { SessionStats } from "../../lib/types";
import { Modal } from "../common/Modal";

interface Props {
  terminalId: string;
  onClose: () => void;
}

/**
 * Claude sessions recorded for the terminal's working directory. Resuming one
 * types `claude --resume <id>` into the shell — the same mechanism a saved
 * connection uses for its ssh line, so what happens is visible in the terminal
 * and the user can edit or abandon it like any other command.
 */
export function ClaudeSessionsDialog({ terminalId, onClose }: Props) {
  const [sessions, setSessions] = useState<SessionStats[] | null>(null);
  const [cwd, setCwd] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      const dir = await ipc.ptyCwd(terminalId).catch(() => null);
      setCwd(dir);
      if (!dir) {
        setSessions([]);
        return;
      }
      setSessions(await ipc.claudeSessions(dir, 15).catch(() => []));
    })();
  }, [terminalId]);

  const resume = (session: SessionStats) => {
    ipc.ptyWrite(terminalId, `claude --resume ${session.id}\r`).catch(() => {});
    onClose();
  };

  return (
    <Modal
      title="Resume a Claude session"
      onClose={onClose}
      width={560}
      footer={<button className="btn" onClick={onClose}>Cancel</button>}
    >
      {cwd && <span className="hint">In {cwd}</span>}
      {sessions === null && <span className="hint">Reading history…</span>}
      {sessions?.length === 0 && (
        <span className="hint">
          No Claude sessions recorded for this directory.
        </span>
      )}
      <div className="session-list">
        {sessions?.map((s) => (
          <button key={s.id} className="session-row" onClick={() => resume(s)}>
            <span className="session-label">
              {s.firstPrompt || <em>Untitled session</em>}
            </span>
            <span className="session-meta">
              {s.updatedAt ? `${new Date(s.updatedAt).toLocaleString()} · ` : ""}
              {s.prompts} prompts · {s.toolCalls} tool calls
              {s.models.length ? ` · ${s.models.join(", ")}` : ""}
            </span>
          </button>
        ))}
      </div>
    </Modal>
  );
}
