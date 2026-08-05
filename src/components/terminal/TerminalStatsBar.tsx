import { useCallback, useEffect, useState } from "react";
import { ipc } from "../../lib/ipc";
import { STAT_PILLS } from "../../lib/statsPills";
import type { SessionStats } from "../../lib/types";
import { useHorizontalWheelScroll } from "../../lib/wheelScroll";
import { useAgentsStore } from "../../stores/agents";
import { useSettingsStore } from "../../stores/settings";

/** How often the numbers are re-read. The backend parses only the bytes added
 *  since the last call, so this stays cheap on a megabyte-scale transcript. */
const POLL_MS = 5000;

interface Props {
  terminalId: string;
}

interface PillValue {
  value: string;
  title: string;
  /** What lands on the clipboard, when it differs from what is displayed. */
  copy?: string;
  tone?: "good" | "bad";
}

function formatDuration(ms: number): string {
  if (ms < 0) return "0s";
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

/** Thousands separators are too wide for a pill; 212.7M reads at a glance. */
function formatCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/**
 * Facts about the terminal and the Claude session running in it, as a strip of
 * pills above the terminal.
 *
 * Which pills appear is the user's choice; the order is the catalogue's, so
 * toggling one never rearranges the rest.
 */
export function TerminalStatsBar({ terminalId }: Props) {
  const enabled = useSettingsStore((s) => s.ui.statsBar);
  const chosen = useSettingsStore((s) => s.ui.statsPills);
  const agent = useAgentsStore((s) => s.byTerminal[terminalId]);
  const [cwd, setCwd] = useState<string | null>(null);
  const [branch, setBranch] = useState<string | null>(null);
  const [stats, setStats] = useState<SessionStats | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  // The strip overflows sideways once enough pills are on; the wheel scrolls it
  // the same way it scrolls the tab bar.
  const wheelRef = useHorizontalWheelScroll<HTMLDivElement>();
  // Elapsed time has to move between polls, or a session clock would visibly
  // jump five seconds at a time.
  const [now, setNow] = useState(() => Date.now());

  const transcriptPath = agent?.transcriptPath ?? null;
  const agentCwd = agent?.cwd ?? null;
  const present = agent?.present ?? false;

  const refresh = useCallback(async () => {
    // The agent reports its own working directory once hooks are connected;
    // otherwise ask the PTY, which is also what keeps this correct after a `cd`.
    let dir = agentCwd;
    if (!dir) dir = await ipc.ptyCwd(terminalId).catch(() => null);
    setCwd(dir);
    setBranch(dir ? await ipc.gitBranch(dir).catch(() => null) : null);

    if (transcriptPath) {
      setStats(await ipc.claudeSessionStats(transcriptPath).catch(() => null));
      return;
    }
    // No hooks: the newest transcript for this directory is the best available
    // guess, and only worth making while a claude process is actually running.
    if (present && dir) {
      const list = await ipc.claudeSessions(dir, 1).catch(() => []);
      setStats(list[0] ?? null);
      return;
    }
    setStats(null);
  }, [terminalId, agentCwd, transcriptPath, present]);

  useEffect(() => {
    if (!enabled) return;
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [refresh, enabled]);

  useEffect(() => {
    if (!enabled || !stats?.startedAt) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [stats?.startedAt, enabled]);

  if (!enabled) return null;

  const values: Record<string, PillValue> = {};

  if (branch) {
    values.branch = {
      value: branch,
      title: `Current git branch in ${cwd ?? "this directory"}`,
    };
  }

  if (stats) {
    const elapsed = stats.startedAt ? now - stats.startedAt : stats.elapsedMs ?? 0;
    values.session = {
      value: formatDuration(elapsed),
      title: `Session open since ${
        stats.startedAt ? new Date(stats.startedAt).toLocaleString() : "unknown"
      }`,
    };
    values.working = {
      value: formatDuration(stats.workingMs),
      title: `Time Claude actually spent working, across ${stats.turns} turns. Longest turn ${formatDuration(stats.longestTurnMs)}.`,
    };
    values.prompts = {
      value: String(stats.prompts),
      // Worth spelling out: the raw "user message" count is more than ten times
      // this, because every tool result comes back on a user-role line.
      title: `Things you asked for. Claude replied ${stats.assistantMessages} times across ${stats.userMessages} user-role lines, most of which are tool results.`,
    };
    values.turns = {
      value: String(stats.turns),
      title: `Completed turns. ${stats.endTurns} ended without a tool call, ${stats.toolUseStops} continued into one.`,
    };
    values.lines = {
      value: `+${formatCount(stats.linesAdded)} −${formatCount(stats.linesRemoved)}`,
      copy: `+${stats.linesAdded} -${stats.linesRemoved}`,
      title: `${stats.linesAdded} added, ${stats.linesRemoved} removed across ${stats.filesTouched} files`,
    };
    values.files = {
      value: String(stats.filesTouched),
      title: stats.topFiles.length
        ? `Most edited:\n${stats.topFiles
            .slice(0, 5)
            .map((f) => `${f.path.split("/").pop()} (${f.edits})`)
            .join("\n")}`
        : "Files touched this session",
    };
    values.tools = {
      value: String(stats.toolCalls),
      title: stats.topTools.length
        ? `Most used:\n${stats.topTools
            .slice(0, 5)
            .map((t) => `${t.name} (${t.count})`)
            .join("\n")}`
        : "Tool calls this session",
    };
    values.toolErrors = {
      value: String(stats.toolErrors),
      title: "Tool calls that came back as errors",
      tone: stats.toolErrors > 0 ? "bad" : undefined,
    };
    values.output = {
      value: formatCount(stats.outputTokens),
      copy: String(stats.outputTokens),
      title: `${stats.outputTokens.toLocaleString()} output tokens`,
    };
    values.input = {
      value: formatCount(stats.inputTokens),
      copy: String(stats.inputTokens),
      title: `${stats.inputTokens.toLocaleString()} fresh input tokens (cache reads counted separately)`,
    };
    values.cacheRead = {
      value: formatCount(stats.cacheReadTokens),
      copy: String(stats.cacheReadTokens),
      title: `${stats.cacheReadTokens.toLocaleString()} tokens served from the prompt cache`,
    };
    values.cacheWrite = {
      value: formatCount(stats.cacheCreationTokens),
      copy: String(stats.cacheCreationTokens),
      title: `${stats.cacheCreationTokens.toLocaleString()} tokens written to the prompt cache`,
    };
    values.compactions = {
      value: String(stats.compactions),
      title: "Times the conversation was compacted — a sign of context pressure",
    };
    values.rework = {
      value: String(stats.userModifiedAfterEdit),
      title: "Files you changed after Claude had edited them",
      tone: stats.userModifiedAfterEdit > 0 ? "bad" : undefined,
    };
    values.interrupted = {
      value: String(stats.interruptedCommands),
      title: "Tool calls that were interrupted before finishing",
    };
    values.sidechain = {
      value: String(stats.sidechainMessages),
      title: "Messages from subagents running inside this session",
    };
    values.size = {
      value: `${(stats.sizeBytes / 1_048_576).toFixed(1)} MB`,
      title: stats.path,
      copy: stats.path,
    };
    if (stats.models.length) {
      values.model = {
        // A session can outlive a model change, and seeing both is the point.
        value: stats.models.map((m) => m.replace(/^claude-/, "")).join(" → "),
        copy: stats.models.join(", "),
        title: stats.models.join("\n"),
      };
    }
    if (stats.claudeVersion) {
      values.version = {
        value: stats.claudeVersion,
        title: `Claude Code version that wrote this session${
          stats.customTitle ? `\n\n${stats.customTitle}` : ""
        }`,
      };
    }
    values.id = {
      value: `${stats.id.slice(0, 8)}…`,
      copy: stats.id,
      title: `${stats.id}\n\nClick to copy the full id — pass it to \`claude --resume\`.`,
    };
  }

  const visible = STAT_PILLS.filter(
    (p) => chosen.includes(p.key) && values[p.key]
  );
  if (visible.length === 0) return null;

  const copy = (key: string, text: string) => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopied(key);
    setTimeout(() => setCopied((c) => (c === key ? null : c)), 1200);
  };

  return (
    <div className="stats-bar" ref={wheelRef}>
      {visible.map((pill) => {
        const v = values[pill.key];
        return (
          <button
            key={pill.key}
            className={`stat-pill${v.tone ? ` ${v.tone}` : ""}`}
            title={`${v.title}\n\nClick to copy`}
            onClick={() => copy(pill.key, v.copy ?? v.value)}
          >
            <span className="stat-pill-label">{pill.label}</span>
            <span className="stat-pill-value">
              {copied === pill.key ? "copied" : v.value}
            </span>
          </button>
        );
      })}
    </div>
  );
}
