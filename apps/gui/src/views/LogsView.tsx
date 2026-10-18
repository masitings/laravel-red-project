import { useMemo, useState } from "react";
import type { LogKind } from "../api/gen/LogKind";
import { useLog, useLogs } from "../api/queries";
import { Badge, Card, ErrorNote } from "../components/ui";

const kindLabel: Record<LogKind, string> = {
  site: "Sites",
  service: "Services",
  php: "PHP",
  daemon: "RED",
};

const levelTone = (level: string | null): "neutral" | "good" | "warn" | "bad" => {
  switch (level) {
    case "emergency":
    case "alert":
    case "critical":
    case "error":
      return "bad";
    case "warning":
    case "notice":
      return "warn";
    case "info":
      return "good";
    default:
      return "neutral";
  }
};

export function LogsView() {
  const logs = useLogs();
  const [selected, setSelected] = useState<string | null>(null);
  const files = logs.data ?? [];
  const groups = (["site", "service", "php", "daemon"] as const)
    .map((kind) => ({ kind, files: files.filter((f) => f.kind === kind) }))
    .filter((g) => g.files.length > 0);
  const first = groups[0]?.files[0]?.id ?? null;
  const current = files.some((f) => f.id === selected) ? selected : first;

  return (
    <Card title="Logs">
      <ErrorNote error={logs.error} />
      {logs.data && files.length === 0 && (
        <p className="text-sm text-stone-500">
          No logs yet. Site logs such as <code>storage/logs/laravel.log</code> show up here.
        </p>
      )}
      {files.length > 0 && (
        <div className="grid grid-cols-[13rem_1fr] gap-4">
          <nav className="space-y-3 text-sm">
            {groups.map((group) => (
              <div key={group.kind}>
                <div className="mb-1 text-xs font-medium uppercase tracking-wide text-stone-400">
                  {kindLabel[group.kind]}
                </div>
                {group.files.map((file) => (
                  <button
                    key={file.id}
                    type="button"
                    onClick={() => setSelected(file.id)}
                    aria-current={current === file.id ? "true" : undefined}
                    title={file.path}
                    className={`block w-full truncate rounded px-2 py-1 text-left ${
                      current === file.id
                        ? "bg-stone-200 font-medium dark:bg-stone-800"
                        : "hover:bg-stone-100 dark:hover:bg-stone-900"
                    }`}
                  >
                    {file.label}
                  </button>
                ))}
              </div>
            ))}
          </nav>
          <div className="min-w-0">{current && <LogEntries id={current} />}</div>
        </div>
      )}
    </Card>
  );
}

function LogEntries({ id }: { id: string }) {
  const [follow, setFollow] = useState(true);
  const [filter, setFilter] = useState("");
  const [level, setLevel] = useState("");
  const log = useLog(id, follow);
  const entries = log.data?.entries ?? [];
  const levels = useMemo(
    () => [...new Set(entries.map((e) => e.level).filter((l): l is string => Boolean(l)))],
    [entries],
  );
  const needle = filter.toLowerCase();
  const shown = entries
    .filter((e) => !level || e.level === level)
    .filter((e) => !needle || e.message.toLowerCase().includes(needle))
    .reverse();

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <input
          aria-label="Filter log"
          placeholder="Filter…"
          className="min-w-40 flex-1 rounded border border-stone-300 bg-transparent px-2 py-1 dark:border-stone-700"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {levels.length > 0 && (
          <select
            aria-label="Level"
            className="rounded border border-stone-300 bg-transparent px-2 py-1 dark:border-stone-700"
            value={level}
            onChange={(e) => setLevel(e.target.value)}
          >
            <option value="">All levels</option>
            {levels.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
        )}
        <label className="flex items-center gap-1 text-xs text-stone-500">
          <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
          live
        </label>
      </div>
      <ErrorNote error={log.error} />
      {log.data && (
        <div className="text-xs text-stone-500">
          {log.data.file.path}
          {log.data.truncated && " · showing the newest entries"}
        </div>
      )}
      {log.data && shown.length === 0 && <p className="text-sm text-stone-500">Nothing to show.</p>}
      <ol className="max-h-[65vh] space-y-1 overflow-y-auto font-mono text-xs">
        {shown.map((entry, i) => (
          <li
            // biome-ignore lint/suspicious/noArrayIndexKey: entries have no ids; position from the end is stable while following
            key={`${shown.length - i}-${entry.time}`}
            className="rounded border border-stone-100 px-2 py-1 dark:border-stone-800"
          >
            {(entry.time || entry.level) && (
              <div className="mb-0.5 flex items-center gap-2 text-stone-500">
                {entry.level && <Badge tone={levelTone(entry.level)}>{entry.level}</Badge>}
                {entry.time && <span>{entry.time}</span>}
                {entry.channel && <span>{entry.channel}</span>}
              </div>
            )}
            <pre className="whitespace-pre-wrap break-all">{entry.message}</pre>
          </li>
        ))}
      </ol>
    </div>
  );
}
