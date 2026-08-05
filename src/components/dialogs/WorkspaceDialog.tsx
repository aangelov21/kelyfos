import { useState } from "react";
import type { Workspace } from "../../lib/types";
import { noAutoAssist } from "../../lib/inputProps";
import { useWorkspacesStore } from "../../stores/workspaces";
import { Modal } from "../common/Modal";
import { TagsField } from "../common/TagsField";
import { VariablesEditor } from "./VariablesEditor";

interface Props {
  workspace: Workspace | null; // null = create new
  onClose: () => void;
}

export function WorkspaceDialog({ workspace, onClose }: Props) {
  const save = useWorkspacesStore((s) => s.save);
  const open = useWorkspacesStore((s) => s.open);
  const [name, setName] = useState(workspace?.name ?? "");
  const [tags, setTags] = useState<string[]>(workspace?.tags ?? []);

  const submit = async () => {
    if (!name.trim()) return;
    const saved = await save({
      id: workspace?.id ?? "",
      name: name.trim(),
      description: workspace?.description ?? "",
      tags,
      icon: workspace?.icon ?? "layout",
      layout: workspace?.layout ?? null,
      createdAt: workspace?.createdAt ?? "",
      updatedAt: workspace?.updatedAt ?? "",
    });
    if (!workspace) open(saved.id);
    onClose();
  };

  return (
    <Modal
      title={workspace ? "Edit workspace" : "New workspace"}
      onClose={onClose}
      width={500}
      footer={
        <>
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={submit} disabled={!name.trim()}>
            Save
          </button>
        </>
      }
    >
      <div className="form-row">
        <label>Name *</label>
        <input {...noAutoAssist} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>
      <div className="form-row">
        <label>Tags</label>
        <TagsField
          colored
          value={tags}
          onChange={setTags}
          placeholder="Pick a tag, or type a new one"
        />
      </div>
      {workspace && (
        <div className="form-row">
          <label>Workspace variables</label>
          <VariablesEditor scope="workspace" ownerId={workspace.id} />
        </div>
      )}
    </Modal>
  );
}
