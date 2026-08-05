import { useEffect, useRef, useState, type FocusEvent } from "react";
import { ipc } from "../../lib/ipc";
import type { Variable, VariableScope } from "../../lib/types";
import { noAutoAssist } from "../../lib/inputProps";

interface Props {
  scope: VariableScope;
  ownerId: string | null;
}

/**
 * Key/value editor bound to one variable scope. Existing rows save on blur; a
 * half-typed new row is committed whenever it is abandoned — focus leaving the
 * row, the scope changing, or the editor unmounting.
 */
export function VariablesEditor({ scope, ownerId }: Props) {
  const [vars, setVars] = useState<Variable[]>([]);
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");

  // Unmount gives no chance to read state, and Escape closes a dialog without
  // ever blurring the inputs, so the half-typed row is mirrored here.
  const pending = useRef({ key: "", value: "" });
  pending.current = { key: newKey, value: newValue };

  /** Writes the pending row, if any, without touching state or reloading. */
  const flush = (toScope: VariableScope, toOwner: string | null) => {
    const { key, value } = pending.current;
    if (!key.trim()) return null;
    pending.current = { key: "", value: "" };
    return ipc
      .variablesSet({ id: "", scope: toScope, ownerId: toOwner, key: key.trim(), value })
      .catch(() => {});
  };

  const load = () => ipc.variablesList(scope, ownerId).then(setVars);
  useEffect(() => {
    load();
    // The owner is captured here rather than read from props at cleanup time:
    // when the scope changes, cleanup runs after the new props are in place and
    // would file the text under the wrong owner.
    return () => {
      flush(scope, ownerId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, ownerId]);

  const add = async () => {
    if (!newKey.trim()) return;
    // Cleared before awaiting: the blur that precedes a click on ＋ would
    // otherwise leave the old text in state long enough for the click to add
    // the same variable a second time. `flush` clears the ref for the same
    // reason, so an unmount mid-save cannot duplicate the row either.
    const done = flush(scope, ownerId);
    setNewKey("");
    setNewValue("");
    await done;
    load();
  };

  // Typing a key and a value and then going straight to Save is the obvious
  // way to use this row, so leaving the row at all commits it. Focus moving
  // between the two inputs is not leaving.
  const onRowBlur = (e: FocusEvent<HTMLDivElement>) => {
    if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
    add();
  };

  const update = async (v: Variable, value: string) => {
    await ipc.variablesSet({ ...v, value });
    load();
  };

  const remove = async (v: Variable) => {
    await ipc.variablesDelete(v.id);
    load();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {vars.map((v) => (
        <div className="kv-row" key={v.id}>
          <input {...noAutoAssist} value={v.key} readOnly />
          <input
            {...noAutoAssist}
            defaultValue={v.value}
            onBlur={(e) => e.target.value !== v.value && update(v, e.target.value)}
          />
          <button title="Delete" onClick={() => remove(v)}>✕</button>
        </div>
      ))}
      <div className="kv-row" onBlur={onRowBlur}>
        <input
          {...noAutoAssist}
          placeholder="KEY"
          value={newKey}
          onChange={(e) => setNewKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <input
          {...noAutoAssist}
          placeholder="value"
          value={newValue}
          onChange={(e) => setNewValue(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <button title="Add" onClick={add}>＋</button>
      </div>
    </div>
  );
}
