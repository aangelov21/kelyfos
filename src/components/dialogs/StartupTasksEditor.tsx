import { useState, type DragEvent } from "react";
import type { StartupTask } from "../../lib/types";
import { noAutoAssist } from "../../lib/inputProps";
import { Select } from "../common/Select";

interface Props {
  tasks: StartupTask[];
  onChange: (tasks: StartupTask[]) => void;
}

const TASK_TYPES: StartupTask["type"][] = ["wait_for_prompt", "command", "wait"];

function defaultTask(type: StartupTask["type"]): StartupTask {
  switch (type) {
    case "command":
      return { type: "command", value: "" };
    case "wait":
      return { type: "wait", milliseconds: 1000 };
    default:
      return { type: "wait_for_prompt" };
  }
}

/** Six-dot grip. Purely decorative: the row's drag events do the work. */
function GripIcon() {
  return (
    <svg viewBox="0 0 10 16" width="10" height="16" aria-hidden="true">
      {[3, 8, 13].map((y) =>
        [2, 8].map((x) => <circle key={`${x}-${y}`} cx={x} cy={y} r="1.3" />)
      )}
    </svg>
  );
}

export function StartupTasksEditor({ tasks, onChange }: Props) {
  // A row is only draggable once its grip is pressed; making rows permanently
  // draggable would steal text selection from the command inputs inside them.
  const [armed, setArmed] = useState<number | null>(null);
  const [from, setFrom] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);

  const setTask = (i: number, task: StartupTask) =>
    onChange(tasks.map((t, idx) => (idx === i ? task : t)));

  const reset = () => {
    setArmed(null);
    setFrom(null);
    setOver(null);
  };

  const onDragStart = (i: number) => (e: DragEvent<HTMLDivElement>) => {
    setFrom(i);
    e.dataTransfer.effectAllowed = "move";
    // Firefox ignores a drag that carries no payload.
    e.dataTransfer.setData("text/plain", String(i));
  };

  const onDragOver = (i: number) => (e: DragEvent<HTMLDivElement>) => {
    if (from === null) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setOver(i);
  };

  const onDrop = (to: number) => (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    if (from === null || from === to) return reset();
    const next = [...tasks];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    onChange(next);
    reset();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {tasks.map((task, i) => {
        // The dragged row lands *at* the hovered index, so the line goes above
        // it when moving up and below it when moving down.
        const marker =
          from === null || over !== i || from === i
            ? ""
            : from > i
              ? " drop-before"
              : " drop-after";
        return (
          <div
            className={`task-row${from === i ? " dragging" : ""}${marker}`}
            key={i}
            draggable={armed === i}
            onDragStart={onDragStart(i)}
            onDragOver={onDragOver(i)}
            onDrop={onDrop(i)}
            onDragEnd={reset}
          >
            <span
              className="drag-grip"
              title="Drag to reorder"
              onMouseDown={() => setArmed(i)}
              onMouseUp={() => setArmed(null)}
            >
              <GripIcon />
            </span>
            <Select
              value={task.type}
              onChange={(t) => setTask(i, defaultTask(t as StartupTask["type"]))}
              options={TASK_TYPES.map((t) => ({ value: t, label: t }))}
            />
            {task.type === "command" && (
              <input
                {...noAutoAssist}
                placeholder="cd ${PROJECT_DIR}"
                value={task.value}
                // A shell command is not prose: autocapitalising the first word
                // or "correcting" a flag would silently break it.
                autoCapitalize="off"
                autoCorrect="off"
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setTask(i, { type: "command", value: e.target.value })}
              />
            )}
            {task.type === "wait" && (
              <input
                {...noAutoAssist}
                type="number"
                min={0}
                style={{ width: 110 }}
                value={task.milliseconds}
                onChange={(e) =>
                  setTask(i, { type: "wait", milliseconds: Number(e.target.value) || 0 })
                }
              />
            )}
            {task.type === "wait" && <span className="hint">ms</span>}
            <button
              className="btn danger"
              title="Remove"
              onClick={() => onChange(tasks.filter((_, idx) => idx !== i))}
            >
              ✕
            </button>
          </div>
        );
      })}
      <div>
        <button
          className="btn"
          onClick={() => onChange([...tasks, defaultTask("command")])}
        >
          + Add task
        </button>
      </div>
    </div>
  );
}
