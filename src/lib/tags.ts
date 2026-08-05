/**
 * Tags are the only colour-coding left on connections and presets: the per-record
 * colour dot said nothing a name did not, while a tag colour is shared across
 * every record wearing it, so the colour actually carries a meaning.
 *
 * The palette is closed on purpose — a tag colour is a label, and the chips have
 * to stay apart from each other at 10px.
 */
export interface TagColor {
  name: string;
  value: string;
}

export const TAG_COLORS: TagColor[] = [
  { name: "Blue", value: "#5eb1ef" },
  { name: "Mint", value: "#7dd3b0" },
  { name: "Amber", value: "#e0a35a" },
  { name: "Rose", value: "#e08bb0" },
  { name: "Periwinkle", value: "#9a9fec" },
];

/** Used for tags saved before colours existed, and for anything unrecognised. */
export const DEFAULT_TAG_COLOR = TAG_COLORS[0].value;

export type TagColorMap = Record<string, string>;

export function tagColor(tag: string, colors: TagColorMap): string {
  return colors[tag] ?? DEFAULT_TAG_COLOR;
}

/**
 * Colour suggested for a tag being created: the first one no existing tag is
 * using, so a set built without ever touching the swatches still comes out
 * distinguishable.
 */
export function nextTagColor(colors: TagColorMap): string {
  const used = new Set(Object.values(colors));
  return TAG_COLORS.find((c) => !used.has(c.value))?.value ?? DEFAULT_TAG_COLOR;
}
