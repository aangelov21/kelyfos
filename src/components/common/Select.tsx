import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * A dropdown that replaces `<select>` everywhere in the app.
 *
 * The reason is the popup, not the closed control. A native select's list is
 * drawn outside the page, and how much of our CSS reaches it depends entirely on
 * the platform: WKWebView hands it to a native menu, and Chromium — which is
 * what WebView2 renders in on Windows — switches from its own dark rendering to
 * an *author-styled* popup the moment any option carries a style of its own,
 * which the font picker's per-family preview does. That popup defaults to a
 * white background, so the list came out white with our light text on it, and no
 * amount of `option { background }` fixes the selected row's system highlight or
 * the scrollbar. Owning the list is the only way to have one dropdown that looks
 * the same in both webviews.
 *
 * Deliberately not a general combobox: no search field, no multi-select, no
 * arbitrary content per row. It does what the selects it replaced did, which is
 * why the API is a flat option list.
 */

export interface SelectOption {
  value: string;
  label: string;
  /**
   * Heading this option belongs under. Consecutive options sharing one get a
   * single heading row above them — how `<optgroup>` read.
   */
  group?: string;
  /**
   * Family the row is drawn in, and the closed control too once it is the
   * selection: the font picker previews each family in itself.
   */
  font?: string;
}

interface Props {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  /** Shown when `value` matches no option — an empty list, or a stale setting. */
  placeholder?: string;
  title?: string;
  className?: string;
  style?: React.CSSProperties;
}

/** Tall enough for eight rows; the font list scrolls inside it. */
const MAX_POPUP_HEIGHT = 296;
/** Gap between the control and its popup, and from the window edges. */
const GAP = 4;
const MARGIN = 8;
/** Typed characters keep matching as one prefix until this much silence. */
const TYPEAHEAD_RESET_MS = 800;

interface PopupPos {
  left: number;
  width: number;
  /** Exactly one of these is set: the popup hangs off the edge it names. */
  top?: number;
  bottom?: number;
  maxHeight: number;
}

/**
 * Place the popup under the control, or over it when the space below cannot
 * take the shorter of the list and its cap and there is more room above.
 * Anchoring upwards by `bottom` rather than a computed `top` keeps the list
 * growing away from the control instead of sliding over it.
 */
function positionPopup(rect: DOMRect, contentHeight: number): PopupPos {
  const below = window.innerHeight - rect.bottom - GAP - MARGIN;
  const above = rect.top - GAP - MARGIN;
  const wanted = Math.min(contentHeight, MAX_POPUP_HEIGHT);
  const base = { left: rect.left, width: rect.width };
  if (below >= wanted || below >= above) {
    return { ...base, top: rect.bottom + GAP, maxHeight: Math.min(wanted, below) };
  }
  return {
    ...base,
    bottom: window.innerHeight - rect.top + GAP,
    maxHeight: Math.min(wanted, above),
  };
}

export function Select({
  value,
  options,
  onChange,
  placeholder,
  title,
  className,
  style,
}: Props) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<PopupPos | null>(null);
  // Row the keyboard is on. Also what the mouse sets by hovering, so there is
  // only ever one highlighted row however the user is driving the list.
  const [active, setActive] = useState(-1);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);
  const typed = useRef({ prefix: "", at: 0 });

  const selectedIndex = options.findIndex((o) => o.value === value);
  const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  /**
   * Place the popup against the control as it is right now. Rows are 34px and a
   * heading 26px, which is close enough to pick a side; the list's real height
   * only ever comes out shorter than the estimate, never taller.
   */
  const place = (): boolean => {
    const rect = triggerRef.current?.getBoundingClientRect();
    if (!rect) return false;
    // Scrolled out of the window, dialog and all: there is nothing to hang off.
    if (rect.bottom < 0 || rect.top > window.innerHeight) return false;
    const groups = new Set(options.map((o) => o.group).filter(Boolean)).size;
    setPos(positionPopup(rect, options.length * 34 + groups * 26 + 8));
    return true;
  };

  const openList = (from = selectedIndex) => {
    if (!options.length || !place()) return;
    setActive(from);
    opening.current = true;
    setOpen(true);
  };

  const commit = (index: number) => {
    setOpen(false);
    const option = options[index];
    if (option && option.value !== value) onChange(option.value);
  };

  // Keep the highlighted row visible. On opening, centre it — the selection in
  // a list as long as the font picker's is usually far off screen; while
  // stepping through, move as little as possible so the list does not jump.
  //
  // Scrolling the list by hand rather than through `scrollIntoView`, which
  // aligns the row in *every* scrollable ancestor: opening the font picker
  // scrolled the settings dialog under it, and the scroll handler below then
  // took that for the user moving the control and closed the list on the spot.
  const opening = useRef(false);
  useLayoutEffect(() => {
    const list = popupRef.current;
    const row = activeRef.current;
    if (!open || !list || !row) return;
    // offsetTop is measured from the popup's border box; clientTop takes the
    // border off, leaving a figure comparable with scrollTop.
    const top = row.offsetTop - list.clientTop;
    const bottom = top + row.offsetHeight;
    if (opening.current) {
      list.scrollTop = top - (list.clientHeight - row.offsetHeight) / 2;
    } else if (top < list.scrollTop) {
      list.scrollTop = top;
    } else if (bottom > list.scrollTop + list.clientHeight) {
      list.scrollTop = bottom - list.clientHeight;
    }
    opening.current = false;
  }, [open, active]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      // The trigger closes through its own click handler; treating its mousedown
      // as "outside" would close and immediately reopen.
      if (popupRef.current?.contains(target) || triggerRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    };
    // The popup is anchored in viewport coordinates, so anything that moves the
    // control has to move the popup with it — and closes it once the control has
    // scrolled out of the window entirely. Following is what makes this robust
    // against the scrolls nobody asked for: focusing a half-visible trigger
    // makes the browser scroll it into view, which used to shut the list again
    // before it had been seen.
    const follow = (e: Event) => {
      // The popup's own scrolling changes nothing about where it belongs, and is
      // why this listens in the capture phase at all: an inner scroll never
      // reaches window in the bubble phase.
      if (e.type === "scroll" && popupRef.current?.contains(e.target as Node)) return;
      if (!place()) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("scroll", follow, true);
    window.addEventListener("resize", follow);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("scroll", follow, true);
      window.removeEventListener("resize", follow);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, options]);

  /** First option starting with `prefix`, searching after `from` and wrapping. */
  const matchPrefix = (prefix: string, from: number): number => {
    for (let i = 1; i <= options.length; i++) {
      const index = (from + i) % options.length;
      if (options[index].label.toLowerCase().startsWith(prefix)) return index;
    }
    return -1;
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setOpen(false);
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (open) commit(active);
      else openList();
      return;
    }
    if (e.key === "Tab") {
      setOpen(false);
      return;
    }

    const step =
      e.key === "ArrowDown" || e.key === "ArrowRight"
        ? 1
        : e.key === "ArrowUp" || e.key === "ArrowLeft"
          ? -1
          : 0;
    if (step !== 0) {
      e.preventDefault();
      if (!open) {
        openList();
        return;
      }
      const from = active < 0 ? (step > 0 ? -1 : options.length) : active;
      setActive(Math.max(0, Math.min(options.length - 1, from + step)));
      return;
    }
    if (open && (e.key === "Home" || e.key === "End")) {
      e.preventDefault();
      setActive(e.key === "Home" ? 0 : options.length - 1);
      return;
    }

    // Typeahead, which a long list needs as much as a native select does.
    // A repeated single letter cycles between the options starting with it —
    // the same behaviour, and what makes one keypress enough to be useful.
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const now = Date.now();
      const fresh = now - typed.current.at > TYPEAHEAD_RESET_MS;
      const prefix = (fresh ? "" : typed.current.prefix) + e.key.toLowerCase();
      typed.current = { prefix, at: now };
      const from = prefix.length === 1 ? (open ? active : selectedIndex) : active - 1;
      const found = matchPrefix(prefix, from);
      if (found < 0) return;
      e.preventDefault();
      if (open) setActive(found);
      else openList(found);
    }
  };

  let lastGroup: string | undefined;

  return (
    <>
      <button
        type="button"
        ref={triggerRef}
        className={`select-trigger${open ? " open" : ""}${className ? ` ${className}` : ""}`}
        style={style}
        title={title}
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => (open ? setOpen(false) : openList())}
        onKeyDown={onKeyDown}
      >
        <span
          className={`select-value${selected ? "" : " placeholder"}`}
          style={selected?.font ? { fontFamily: selected.font } : undefined}
        >
          {selected?.label ?? placeholder ?? ""}
        </span>
        <span className="select-chevron" aria-hidden="true" />
      </button>

      {open && pos && (
        <div
          ref={popupRef}
          className="select-popup"
          role="listbox"
          style={{
            left: pos.left,
            width: pos.width,
            top: pos.top,
            bottom: pos.bottom,
            maxHeight: pos.maxHeight,
          }}
        >
          {options.map((option, i) => {
            const heading = option.group && option.group !== lastGroup ? option.group : null;
            lastGroup = option.group;
            return (
              <Fragment key={option.value}>
                {heading && <div className="select-group">{heading}</div>}
                <div
                  ref={i === active ? activeRef : undefined}
                  className={`select-option${i === active ? " active" : ""}${
                    option.value === value ? " selected" : ""
                  }`}
                  role="option"
                  aria-selected={option.value === value}
                  style={option.font ? { fontFamily: option.font } : undefined}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => commit(i)}
                >
                  {option.label}
                </div>
              </Fragment>
            );
          })}
        </div>
      )}
    </>
  );
}
