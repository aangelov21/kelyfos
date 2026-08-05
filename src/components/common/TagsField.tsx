import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useConnectionsStore } from "../../stores/connections";
import { usePresetsStore } from "../../stores/presets";
import { useSettingsStore } from "../../stores/settings";
import { useWorkspacesStore } from "../../stores/workspaces";
import { TAG_COLORS, nextTagColor, tagColor } from "../../lib/tags";
import { noAutoAssist } from "../../lib/inputProps";

interface Props {
  value: string[];
  onChange: (tags: string[]) => void;
  placeholder?: string;
  /**
   * Treat the entries as tags: colour them, offer the ones already in use, and
   * let a new one pick a colour. Off for the fields that merely reuse the chip
   * editor for free text (error patterns), where none of that means anything.
   */
  colored?: boolean;
}

/**
 * Chip editor for tags. The text being typed is local state and only committed
 * tags go to the parent — an earlier version kept a joined string in `value` and
 * re-split it on every keystroke, so the comma meant to separate tags was
 * stripped before it could.
 */
export function TagsField({ value, onChange, placeholder, colored }: Props) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  /** Which chip has its colour palette open, if any. */
  const [picking, setPicking] = useState<string | null>(null);
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const connections = useConnectionsStore((s) => s.connections);
  const presets = usePresetsStore((s) => s.presets);
  const workspaces = useWorkspacesStore((s) => s.workspaces);
  const tagColors = useSettingsStore((s) => s.tagColors);
  const setTagColor = useSettingsStore((s) => s.setTagColor);

  // Every tag anyone is wearing, plus any that only exist as a saved colour.
  // A tag is not a record, so this is the whole of its existence.
  const known = useMemo(() => {
    if (!colored) return [];
    const all = new Set<string>(Object.keys(tagColors));
    for (const c of connections) c.tags.forEach((t) => all.add(t));
    for (const p of presets) p.tags.forEach((t) => all.add(t));
    for (const w of workspaces) w.tags.forEach((t) => all.add(t));
    return [...all].sort((a, b) => a.localeCompare(b));
  }, [colored, tagColors, connections, presets, workspaces]);

  const query = text.trim().toLowerCase();
  const suggestions = known.filter(
    (t) => !value.includes(t) && t.toLowerCase().includes(query)
  );
  const isNew = !!query && !known.some((t) => t.toLowerCase() === query);

  const add = (tag: string, color?: string) => {
    // A click inside the menu blurs the input first, arming the commit below;
    // that commit would re-add the half-typed text this click is replacing.
    keepOpen();
    setText("");
    setPicking(null);
    if (!tag) return;
    if (color) setTagColor(tag, color);
    if (!value.includes(tag)) onChange([...value, tag]);
  };

  const commit = (raw: string) => {
    const tag = raw.trim();
    if (!tag) {
      setText("");
      return;
    }
    // A tag typed rather than picked still needs a colour, so it gets the first
    // unused one — the swatches are for overriding that, not for supplying it.
    const existing = known.find((t) => t.toLowerCase() === tag.toLowerCase());
    add(existing ?? tag, existing || !colored ? undefined : nextTagColor(tagColors));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" || e.key === ",") {
      // Enter would submit nothing here, but a comma would otherwise be typed
      // into the field it is supposed to act as a separator for.
      e.preventDefault();
      commit(text);
      return;
    }
    if (e.key === "Escape" && open) {
      e.preventDefault();
      setOpen(false);
      return;
    }
    // Backspace on an empty field takes back the previous chip, which is the
    // only way to correct one without a mouse.
    if (e.key === "Backspace" && !text && value.length) {
      onChange(value.slice(0, -1));
    }
  };

  // Leaving the field for anywhere but its own menu commits what was typed.
  // The menu suppresses the blur itself (mousedown default), so the delay here
  // only covers the swatch popovers, which sit outside the input's subtree.
  const onBlur = () => {
    blurTimer.current = setTimeout(() => {
      commit(text);
      setOpen(false);
      setPicking(null);
    }, 120);
  };
  const keepOpen = () => {
    if (blurTimer.current) clearTimeout(blurTimer.current);
  };

  return (
    <div className="tags-field-wrap">
      <div className="tags-field">
        {value.map((tag) => {
          const color = colored ? tagColor(tag, tagColors) : undefined;
          return (
            <span
              className={`tag-chip${colored ? " colored" : ""}`}
              key={tag}
              style={color ? { "--tag-color": color } as React.CSSProperties : undefined}
            >
              {colored && (
                <button
                  className="tag-chip-dot"
                  title={`Recolor ${tag}`}
                  // Without this the input blurs, and the commit that schedules
                  // would close the palette this click just opened.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => setPicking(picking === tag ? null : tag)}
                />
              )}
              {tag}
              <button
                title={`Remove ${tag}`}
                onClick={() => onChange(value.filter((t) => t !== tag))}
              >
                ×
              </button>
              {picking === tag && (
                <span
                  className="tag-palette floating"
                  onMouseDown={(e) => e.preventDefault()}
                >
                  {TAG_COLORS.map((c) => (
                    <button
                      key={c.value}
                      title={c.name}
                      className={`tag-swatch${
                        tagColor(tag, tagColors) === c.value ? " selected" : ""
                      }`}
                      style={{ background: c.value }}
                      onClick={() => {
                        setTagColor(tag, c.value);
                        setPicking(null);
                      }}
                    />
                  ))}
                </span>
              )}
            </span>
          );
        })}
        <input
          {...noAutoAssist}
          value={text}
          placeholder={value.length ? "" : placeholder ?? "Add a tag…"}
          autoCapitalize="off"
          autoCorrect="off"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onKeyDown}
          // Leaving the field with half-typed text would silently drop it.
          onBlur={onBlur}
        />
      </div>

      {colored && open && (suggestions.length > 0 || isNew) && (
        <div className="tag-menu" onMouseDown={(e) => e.preventDefault()}>
          {suggestions.map((t) => (
            <button className="tag-menu-item" key={t} onClick={() => add(t)}>
              <span
                className="tag-menu-dot"
                style={{ background: tagColor(t, tagColors) }}
              />
              {t}
            </button>
          ))}
          {isNew && (
            <div className="tag-menu-new">
              <span className="tag-menu-new-label">
                New tag “{text.trim()}”
              </span>
              <span className="tag-palette">
                {TAG_COLORS.map((c) => (
                  <button
                    key={c.value}
                    title={c.name}
                    className="tag-swatch"
                    style={{ background: c.value }}
                    onClick={() => add(text.trim(), c.value)}
                  />
                ))}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
