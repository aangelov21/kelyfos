import type { Preset } from "./types";

/**
 * MIME type marking a drag started from a sidebar preset. Mirrors
 * CONNECTION_DRAG_TYPE: only the type is readable during dragenter/dragover,
 * so the preset itself is parked in module state and the payload is a marker.
 */
export const PRESET_DRAG_TYPE = "application/x-kelyfos-preset";

let dragged: Preset | null = null;

export function setDraggedPreset(p: Preset | null) {
  dragged = p;
}

export function getDraggedPreset(): Preset | null {
  return dragged;
}

/** True when the given drag event carries a sidebar preset. */
export function isPresetDrag(dataTransfer: DataTransfer | null): boolean {
  return !!dataTransfer && Array.from(dataTransfer.types).includes(PRESET_DRAG_TYPE);
}
