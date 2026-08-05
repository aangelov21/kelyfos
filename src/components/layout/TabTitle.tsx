import { useEffect, useRef, useState } from "react";

/** Pixels per second the title travels while scrolling on hover. */
const SCROLL_SPEED = 45;
/**
 * A pure distance/speed duration makes a title that overflows by a few pixels
 * snap back and forth. Every roll gets a fixed lead-in and a floor so short and
 * long titles read at a comparable pace.
 */
const BASE_DURATION = 1.4;
const MIN_DURATION = 1;
const MAX_DURATION = 4;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Tab label capped at 200px. A title that doesn't fit is faded out at its
 * trailing edge (rather than cut with an ellipsis) and scrolls its full text
 * while the tab is hovered. Overflow can't be expressed in CSS, so the
 * distance is measured here and handed to the animation as a custom property.
 */
export function TabTitle({ name }: { name: string }) {
  const outerRef = useRef<HTMLSpanElement>(null);
  const innerRef = useRef<HTMLSpanElement>(null);
  const [overflow, setOverflow] = useState(0);

  useEffect(() => {
    const outer = outerRef.current;
    const inner = innerRef.current;
    if (!outer || !inner) return;
    setOverflow(Math.max(0, inner.scrollWidth - outer.clientWidth));
  }, [name]);

  const duration = clamp(
    BASE_DURATION + overflow / SCROLL_SPEED,
    MIN_DURATION,
    MAX_DURATION
  );

  const style = overflow
    ? ({
        "--tab-title-scroll": `${overflow}px`,
        "--tab-title-duration": `${duration.toFixed(2)}s`,
      } as React.CSSProperties)
    : undefined;

  return (
    <span
      ref={outerRef}
      className={`tab-title${overflow ? " overflowing" : ""}`}
      style={style}
      title={name}
    >
      <span ref={innerRef} className="tab-title-inner">
        {name}
      </span>
    </span>
  );
}
