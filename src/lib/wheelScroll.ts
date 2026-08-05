import { useCallback, useRef } from "react";

/**
 * Turn the vertical wheel into horizontal scrolling on a strip that only
 * overflows sideways.
 *
 * Matches what FlexLayout does for the tab bar — including the ×40 for
 * line-mode wheels, which some mice report instead of pixels — so the stats bar
 * and the tabs above it behave identically under the same gesture.
 *
 * Returns a callback ref rather than taking one: the element it attaches to
 * comes and goes with conditional rendering, and a callback ref is told about
 * both, where a `useEffect` on a ref would miss the element appearing later.
 */
export function useHorizontalWheelScroll<T extends HTMLElement>() {
  const detach = useRef<(() => void) | null>(null);

  return useCallback((el: T | null) => {
    detach.current?.();
    detach.current = null;
    if (!el) return;

    const onWheel = (event: WheelEvent) => {
      // A trackpad's sideways swipe arrives as deltaX and already scrolls this
      // element natively; only the vertical wheel needs translating.
      if (!event.deltaY) return;
      const max = el.scrollWidth - el.clientWidth;
      if (max <= 0) return;

      const step = event.deltaMode === 1 ? event.deltaY * 40 : event.deltaY;
      const next = Math.max(0, Math.min(max, el.scrollLeft + step));
      // Already hard against an edge: let the event through rather than
      // swallowing a gesture that was meant for something else.
      if (next === el.scrollLeft) return;

      el.scrollLeft = next;
      event.preventDefault();
    };

    // Explicitly non-passive: React registers wheel handlers as passive, where
    // preventDefault is ignored, so this has to be a native listener.
    el.addEventListener("wheel", onWheel, { passive: false });
    detach.current = () => el.removeEventListener("wheel", onWheel);
  }, []);
}
