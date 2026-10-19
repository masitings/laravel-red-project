import { ScrollTextIcon } from "lucide-react";
import { useMemo, useState } from "react";
import type { LogKind } from "@/api/gen/LogKind";
import { useLog, useLogs } from "@/api/queries";
import { Empty, ErrorNote, PageHeader } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

const kindLabel: Record<LogKind, string> = {
  site: "Sites",
  service: "Services",
  php: "PHP",
  daemon: "RED",
};

const levelVariant = (
  level: string | null,
): "secondary" | "success" | "warning" | "destructive" => {
  switch (level) {
    case "emergency":
    case "alert":
    case "critical":
    case "error":
      return "destructive";
    case "warning":
    case "notice":
      return "warning";
    case "info":
      return "success";
    default:
      return "secondary";
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
    <div className="space-y-6">
      <PageHeader
        title="Logs"
        description="RED, service, PHP-FPM and site logs such as storage/logs/laravel.log."
      />
      <ErrorNote error={logs.error} />
      {logs.data && files.length === 0 && (
        <Empty icon={<ScrollTextIcon />}>
          <p>No logs yet. Site logs show up here as soon as they are written.</p>
        </Empty>
      )}
      {files.length > 0 && (
        <Card className="py-0">
          <CardContent className="grid grid-cols-[14rem_1fr] gap-0 px-0">
            <nav aria-label="Log files" className="space-y-4 border-r p-3 text-sm">
              {groups.map((group) => (
                <div key={group.kind}>
                  <div className="mb-1 px-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
                    {kindLabel[group.kind]}
                  </div>
                  {group.files.map((file) => (
                    <button
                      key={file.id}
                      type="button"
                      onClick={() => setSelected(file.id)}
                      aria-current={current === file.id ? "true" : undefined}
                      title={file.path}
                      className={cn(
                        "block w-full truncate rounded-md px-2 py-1 text-left",
                        current === file.id
                          ? "bg-accent font-medium text-accent-foreground"
                          : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                      )}
                    >
                      {file.label}
                    </button>
                  ))}
                </div>
              ))}
            </nav>
            <div className="min-w-0 p-4">{current && <LogEntries id={current} />}</div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

const ALL = "all";

/** The newest entries of one log, filterable, following new ones. */
export function LogEntries({ id }: { id: string }) {
  const [follow, setFollow] = useState(true);
  const [filter, setFilter] = useState("");
  const [level, setLevel] = useState(ALL);
  const log = useLog(id, follow);
  const entries = log.data?.entries ?? [];
  const levels = useMemo(
    () => [...new Set(entries.map((e) => e.level).filter((l): l is string => Boolean(l)))],
    [entries],
  );
  const needle = filter.toLowerCase();
  const shown = entries
    .filter((e) => level === ALL || e.level === level)
    .filter((e) => !needle || e.message.toLowerCase().includes(needle))
    .reverse();

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <Input
          aria-label="Filter log"
          placeholder="Filter…"
          className="h-8 min-w-40 flex-1"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {levels.length > 0 && (
          <Select value={level} onValueChange={setLevel}>
            <SelectTrigger size="sm" className="w-36" aria-label="Level">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>All levels</SelectItem>
              {levels.map((l) => (
                <SelectItem key={l} value={l}>
                  {l}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
        <div className="flex items-center gap-2">
          <Switch id={`follow-${id}`} checked={follow} onCheckedChange={setFollow} />
          <Label htmlFor={`follow-${id}`} className="text-xs text-muted-foreground">
            Live
          </Label>
        </div>
      </div>
      <ErrorNote error={log.error} />
      {log.data && (
        <div className="truncate text-xs text-muted-foreground">
          {log.data.file.path}
          {log.data.truncated && " · showing the newest entries"}
        </div>
      )}
      {log.data && shown.length === 0 && (
        <p className="text-sm text-muted-foreground">Nothing to show.</p>
      )}
      <ol className="max-h-[65vh] space-y-1 overflow-y-auto font-mono text-xs">
        {shown.map((entry, i) => (
          <li
            // biome-ignore lint/suspicious/noArrayIndexKey: entries have no ids; position from the end is stable while following
            key={`${shown.length - i}-${entry.time}`}
            className="rounded-md border px-2 py-1"
          >
            {(entry.time || entry.level) && (
              <div className="mb-0.5 flex items-center gap-2 text-muted-foreground">
                {entry.level && <Badge variant={levelVariant(entry.level)}>{entry.level}</Badge>}
                {entry.time && <span>{entry.time}</span>}
                {entry.channel && <span>{entry.channel}</span>}
              </div>
            )}
            <pre className="break-all whitespace-pre-wrap">{entry.message}</pre>
          </li>
        ))}
      </ol>
    </div>
  );
}
