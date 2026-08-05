import type { Connection } from "./types";

/**
 * MIME type that marks a drag started from a sidebar connection. Only the type
 * is readable during dragenter/dragover (the browser hides getData() until the
 * drop), so the connection itself is parked in module state and the payload is
 * used purely as a marker.
 */
export const CONNECTION_DRAG_TYPE = "application/x-kelyfos-connection";

let dragged: Connection | null = null;

export function setDraggedConnection(c: Connection | null) {
  dragged = c;
}

export function getDraggedConnection(): Connection | null {
  return dragged;
}

/** True when the given drag event carries a sidebar connection. */
export function isConnectionDrag(dataTransfer: DataTransfer | null): boolean {
  return !!dataTransfer && Array.from(dataTransfer.types).includes(CONNECTION_DRAG_TYPE);
}
