import type { TerminalDef } from "../../lib/types";
import { useTerminalsStore } from "../../stores/terminals";
import { TerminalStatsBar } from "../terminal/TerminalStatsBar";
import { XtermView } from "../terminal/XtermView";

/** Actions live in the layout tab; the pane is the terminal plus the strip of
 *  facts about it, which sits outside the xterm surface so it never competes
 *  with the shell for the top row. */
export function TerminalPane({ terminal }: { terminal: TerminalDef }) {
  const epoch = useTerminalsStore((s) => s.epochs[terminal.id] ?? 0);
  return (
    <div className="terminal-pane">
      <TerminalStatsBar terminalId={terminal.id} />
      <XtermView
        key={epoch}
        terminalId={terminal.id}
        connectionId={terminal.connectionId}
        workspaceId={terminal.workspaceId}
        shell={terminal.shell}
      />
    </div>
  );
}
