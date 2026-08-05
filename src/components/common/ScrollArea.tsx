import { ReactNode, useCallback, useEffect, useRef, useState } from "react";

interface Props {
  /** Goes on the scrolling element, not the wrapper, so the caller's own layout
   *  rules (`.rail-content` is a flex column of items) still apply to it. */
  className?: string;
  children: ReactNode;
}

interface Thumb {
  top: number;
  height: number;
}

/** Never let the thumb shrink to a sliver on a very long list — below this it
 *  stops being a grab target. */
const MIN_THUMB = 24;

/**
 * A scroll container with a drawn scrollbar instead of the native one.
 *
 * The native bar is styleable (`::-webkit-scrollbar-*`) but not animatable —
 * Chromium, and therefore WebView2 on Windows, ignores transitions on those
 * pseudo-elements, so a hover fade there snapped on and off. This is a plain
 * absolutely-positioned div, so it fades like anything else and behaves the
 * same on both platforms.
 *
 * The visibility rule lives in CSS (`.sidebar:hover`), not here: it is a hover
 * state, and reproducing it with pointerenter/pointerleave listeners would
 * desync the moment a dialog opens over the sidebar.
 */
export function ScrollArea({ className, children }: Props) {
  const viewport = useRef<HTMLDivElement>(null);
  // null means "not scrollable" — the thumb is not rendered at all, rather than
  // rendered full-height, which would read as a bar that cannot be moved.
  const [thumb, setThumb] = useState<Thumb | null>(null);
  const [dragging, setDragging] = useState(false);

  const measure = useCallback(() => {
    const el = viewport.current;
    if (!el) return;
    const track = el.clientHeight;
    const content = el.scrollHeight;
    // The 1px slack absorbs sub-pixel layout, which otherwise reports a
    // permanently scrollable element and leaves a thumb on a list that fits.
    if (content <= track + 1) {
      setThumb((prev) => (prev === null ? prev : null));
      return;
    }
    const height = Math.max(MIN_THUMB, (track / content) * track);
    const top = (el.scrollTop / (content - track)) * (track - height);
    setThumb((prev) =>
      prev && Math.abs(prev.top - top) < 0.5 && Math.abs(prev.height - height) < 0.5
        ? prev
        : { top, height }
    );
  }, []);

  useEffect(() => {
    const el = viewport.current;
    if (!el) return;
    measure();
    el.addEventListener("scroll", measure, { passive: true });
    // Two observers, because the two things that change the thumb are
    // independent: the pane resizing, and rows being added or removed by a
    // store update (a new workspace, a filtered search) without any resize.
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    for (const child of Array.from(el.children)) ro.observe(child);
    const mo = new MutationObserver(() => {
      for (const child of Array.from(el.children)) ro.observe(child);
      measure();
    });
    mo.observe(el, { childList: true, subtree: true });
    return () => {
      el.removeEventListener("scroll", measure);
      ro.disconnect();
      mo.disconnect();
    };
  }, [measure]);

  const startDrag = (e: React.PointerEvent) => {
    const el = viewport.current;
    if (!el || !thumb) return;
    // Otherwise the drag selects text and the pointer leaves the thumb behind.
    e.preventDefault();
    const startY = e.clientY;
    const startTop = el.scrollTop;
    const track = el.clientHeight;
    // Pointer travel → scroll travel. The thumb can only move (track - height)
    // pixels, and that has to cover the whole overflow.
    const ratio = (el.scrollHeight - track) / (track - thumb.height);

    const move = (ev: PointerEvent) => {
      el.scrollTop = startTop + (ev.clientY - startY) * ratio;
    };
    const stop = () => {
      setDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
    setDragging(true);
    // On window, not the thumb: a fast drag outruns the pointer and the gesture
    // must survive the cursor leaving the 7px-wide element.
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
  };

  return (
    <div className={`scroll-area${dragging ? " dragging" : ""}`}>
      <div ref={viewport} className={`scroll-viewport${className ? ` ${className}` : ""}`}>
        {children}
      </div>
      {thumb && (
        <div
          className="scroll-thumb"
          style={{ top: thumb.top, height: thumb.height }}
          onPointerDown={startDrag}
        />
      )}
    </div>
  );
}
