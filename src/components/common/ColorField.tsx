import { useEffect, useState } from "react";
import { noAutoAssist } from "../../lib/inputProps";

interface Props {
  /** Current override, or undefined when the theme's value is in use. */
  value: string | undefined;
  /** Active theme's value, shown in the swatch while no override is set. */
  fallback: string;
  onChange: (value: string | undefined) => void;
}

const HEX = /^#[0-9a-fA-F]{6}$/;

/** Normalizes #abc to #aabbcc so the native swatch accepts it. */
function expand(hex: string): string {
  const short = /^#([0-9a-fA-F])([0-9a-fA-F])([0-9a-fA-F])$/.exec(hex);
  return short ? `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}` : hex;
}

/**
 * Swatch + hex entry for an optional color override. `<input type="color">`
 * has no empty state, so "no override" is represented by showing the theme's
 * color and offering an explicit reset rather than by blanking the swatch.
 */
export function ColorField({ value, fallback, onChange }: Props) {
  const [text, setText] = useState(value ?? "");

  // Follow external changes (theme switch, reset) without fighting typing.
  useEffect(() => setText(value ?? ""), [value]);

  const commit = (raw: string) => {
    setText(raw);
    if (!raw.trim()) {
      onChange(undefined);
      return;
    }
    const hex = expand(raw.trim());
    // Ignore half-typed values; the field keeps them until they parse.
    if (HEX.test(hex)) onChange(hex);
  };

  const invalid = text.trim() !== "" && !HEX.test(expand(text.trim()));

  return (
    <div className="color-field">
      <input
        type="color"
        aria-label="Color picker"
        value={expand(value ?? fallback)}
        onChange={(e) => {
          setText(e.target.value);
          onChange(e.target.value);
        }}
      />
      <input
        {...noAutoAssist}
        className={invalid ? "invalid" : undefined}
        value={text}
        placeholder={fallback}
        spellCheck={false}
        onChange={(e) => commit(e.target.value)}
      />
      {/* Present only while an override exists; dropping it restores whatever
          the active theme defines, which is what makes the button disappear. */}
      {value !== undefined && (
        <button
          className="btn"
          title={`Back to ${fallback}`}
          onClick={() => onChange(undefined)}
        >
          Reset
        </button>
      )}
    </div>
  );
}
