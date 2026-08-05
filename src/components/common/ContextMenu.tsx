import { useEffect, useRef } from "react";

export interface MenuItem {
  label: string;
  danger?: boolean;
  separator?: boolean;
  /** A non-action row (heading); clicking it just dismisses the menu. */
  muted?: boolean;
  /** Marks the row that is already active. */
  selected?: boolean;
  onClick?: () => void;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", handler);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", handler);
      window.removeEventListener("keydown", esc);
    };
  }, [onClose]);

  // Keep the menu inside the viewport.
  const style: React.CSSProperties = {
    left: Math.min(x, window.innerWidth - 180),
    top: Math.min(y, window.innerHeight - items.length * 30 - 16),
  };

  return (
    <div className="context-menu" style={style} ref={ref}>
      {items.map((item, i) =>
        item.separator ? (
          <div key={i} className="context-menu-sep" />
        ) : (
          <div
            key={i}
            className={`context-menu-item${item.danger ? " danger" : ""}${
              item.muted ? " muted" : ""
            }${item.selected ? " selected" : ""}`}
            onClick={() => {
              onClose();
              item.onClick?.();
            }}
          >
            {item.label}
          </div>
        )
      )}
    </div>
  );
}
