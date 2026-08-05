import { useEffect, useRef, useState } from "react";

/** ms before a tooltip appears — far snappier than the native ~700ms. */
const DELAY = 120;
/** Keep the bubble this far from the window edges. */
const MARGIN = 6;

interface Tip {
  text: string;
  x: number;
  y: number;
  /** true = bubble sits below the target (arrow points up). */
  below: boolean;
}

/**
 * A single app-wide tooltip. Rather than touch the 20-odd `title=""` call
 * sites, it intercepts them: on hover it lifts the element's `title` into a
 * data attribute (suppressing the OS bubble) and renders our own, restoring
 * the attribute on leave so React and accessibility keep working.
 */
export function TooltipHost() {
  const [tip, setTip] = useState<Tip | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const bubble = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const clear = () => {
      window.clearTimeout(timer.current);
      setTip(null);
    };

    const onOver = (e: MouseEvent) => {
      const el = (e.target as HTMLElement | null)?.closest<HTMLElement>("[title]");
      window.clearTimeout(timer.current);
      if (!el) {
        setTip(null);
        return;
      }
      const text = el.getAttribute("title") ?? "";
      if (!text.trim()) return;
      // Move title out of the DOM so the native tooltip never fires.
      el.setAttribute("data-tip", text);
      el.removeAttribute("title");

      const restore = () => {
        if (el.getAttribute("data-tip") !== null && !el.getAttribute("title")) {
          el.setAttribute("title", el.getAttribute("data-tip") ?? "");
          el.removeAttribute("data-tip");
        }
        el.removeEventListener("mouseleave", restore);
      };
      el.addEventListener("mouseleave", restore);

      timer.current = window.setTimeout(() => {
        const r = el.getBoundingClientRect();
        const below = r.top < 60;
        setTip({
          text,
          x: r.left + r.width / 2,
          y: below ? r.bottom + 8 : r.top - 8,
          below,
        });
      }, DELAY);
    };

    document.addEventListener("mouseover", onOver);
    document.addEventListener("mousedown", clear);
    window.addEventListener("scroll", clear, true);
    return () => {
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("mousedown", clear);
      window.removeEventListener("scroll", clear, true);
      window.clearTimeout(timer.current);
    };
  }, []);

  // Clamp horizontally into the viewport once the bubble has a measured width.
  useEffect(() => {
    if (!tip || !bubble.current) return;
    const w = bubble.current.offsetWidth;
    const half = w / 2;
    const min = MARGIN + half;
    const max = window.innerWidth - MARGIN - half;
    const clamped = Math.max(min, Math.min(max, tip.x));
    if (clamped !== tip.x) setTip({ ...tip, x: clamped });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tip?.text, tip?.x]);

  if (!tip) return null;

  return (
    <div
      ref={bubble}
      className={`app-tooltip ${tip.below ? "below" : "above"}`}
      style={{ left: tip.x, top: tip.y }}
      role="tooltip"
    >
      {tip.text}
    </div>
  );
}
