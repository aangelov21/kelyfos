/**
 * Marks a terminal as running Claude Code. Sits next to the status dot rather
 * than replacing it: the dot says what is happening, this says who is doing it,
 * and a tab can be busy for reasons that have nothing to do with an agent.
 *
 * A named pill rather than a bare glyph — at this size an unlabelled mark is a
 * shape the user has to have learnt, and there is room to just say it.
 */
export function ClaudeMark({ title }: { title?: string }) {
  return (
    <span className="claude-mark" title={title}>
      Claude
      <svg
        viewBox="0 0 24 24"
        width="10"
        height="10"
        aria-hidden="true"
        focusable="false"
      >
        {/* A six-armed burst: legible at 10px, where anything with interior
            detail turns into a smudge. */}
        <g
          stroke="currentColor"
          strokeWidth="2.6"
          strokeLinecap="round"
          fill="none"
        >
          <path d="M12 3.5v17" />
          <path d="M4.6 7.75l14.8 8.5" />
          <path d="M4.6 16.25l14.8-8.5" />
        </g>
      </svg>
    </span>
  );
}
