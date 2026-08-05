/**
 * The tabstrip overflow button: a terminal glyph with the number of terminals
 * in the tabset printed on its screen. The count sits at 38% of the height,
 * which is the centre of the screen area in this glyph.
 */
export function TerminalCountIcon({ count }: { count: number }) {
  return (
    <span className="terminal-count">
      <svg viewBox="0 -960 960 960" width="20" height="20" fill="currentColor" aria-hidden>
        <path d="M55.39-150.77v-60h849.22v60H55.39Zm116.92-100q-30.31 0-51.31-21-21-21-21-51.31v-415.38q0-30.31 21-51.31 21-21 51.31-21h615.38q30.31 0 51.31 21 21 21 21 51.31v415.38q0 30.31-21 51.31-21 21-51.31 21H172.31Zm0-60h615.38q4.62 0 8.46-3.84 3.85-3.85 3.85-8.47v-415.38q0-4.62-3.85-8.46-3.84-3.85-8.46-3.85H172.31q-4.62 0-8.46 3.85-3.85 3.84-3.85 8.46v415.38q0 4.62 3.85 8.47 3.84 3.84 8.46 3.84Zm-12.31 0v-440 440Z" />
      </svg>
      <span className="terminal-count-value">{count}</span>
    </span>
  );
}
