import { useSettingsStore } from "../../stores/settings";
import { tagColor } from "../../lib/tags";

/**
 * A tag as it appears outside the editor: read-only, and tinted with the colour
 * the tag itself carries. Records no longer have colours of their own, so this
 * is the only colour a sidebar row shows.
 */
export function TagChip({ tag }: { tag: string }) {
  const tagColors = useSettingsStore((s) => s.tagColors);
  return (
    <span
      className="tag"
      style={{ "--tag-color": tagColor(tag, tagColors) } as React.CSSProperties}
    >
      {tag}
    </span>
  );
}
