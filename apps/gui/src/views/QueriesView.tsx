import { CopyIcon, ScanSearchIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import type { QueryEntry } from "@/api/gen/QueryEntry";
import { useQueryLog, useQueryLogActions } from "@/api/queries";
import { Empty, ErrorNote, PageHeader } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

/** Queries slower than this are flagged. */
const SLOW_MS = 100;

type Group = {
  key: string;
  request: string;
  received: number;
  queries: QueryEntry[];
  total: number;
  /** SQL (with placeholders) run more than once in the request. */
  repeated: Map<string, number>;
};

/** Queries grouped per request, newest request first. */
export function groupQueries(queries: QueryEntry[]): Group[] {
  const groups = new Map<string, Group>();
  for (const query of queries) {
    const key = query.request_id ?? `q${query.id}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        key,
        request: query.request ?? "",
        received: query.received,
        queries: [],
        total: 0,
        repeated: new Map(),
      };
      groups.set(key, group);
    }
    group.queries.push(query);
    group.total += query.time_ms;
  }
  for (const group of groups.values()) {
    const counts = new Map<string, number>();
    for (const q of group.queries) counts.set(q.sql, (counts.get(q.sql) ?? 0) + 1);
    for (const [sql, n] of counts) if (n > 1) group.repeated.set(sql, n);
  }
  return [...groups.values()].reverse();
}

/** `app/Http/Controllers/A.php` from an absolute path, when it has an app dir. */
function shortPath(path: string): string {
  for (const dir of ["/app/", "/routes/", "/database/", "/resources/", "/config/"]) {
    const at = path.indexOf(dir);
    if (at >= 0) return path.slice(at + 1);
  }
  return path;
}

export function QueriesView() {
  const log = useQueryLog();
  const actions = useQueryLogActions();
  const [filter, setFilter] = useState("");
  const data = log.data;
  const groups = useMemo(() => {
    const needle = filter.toLowerCase();
    const queries = (data?.queries ?? []).filter(
      (q) =>
        !needle ||
        q.bound.toLowerCase().includes(needle) ||
        (q.request ?? "").toLowerCase().includes(needle),
    );
    return groupQueries(queries);
  }, [data, filter]);

  return (
    <div className="space-y-6">
      <PageHeader
        title="Queries"
        description="Database queries your Laravel sites run, live, with where in the app they came from. No package needed."
        actions={
          data && (
            <>
              <div className="flex items-center gap-2">
                <Switch
                  id="capture-queries"
                  checked={data.enabled}
                  onCheckedChange={(on) => actions.setEnabled.mutate(on)}
                />
                <Label htmlFor="capture-queries">Capture queries</Label>
              </div>
              {data.queries.length > 0 && (
                <Button variant="outline" onClick={() => actions.clear.mutate()}>
                  <Trash2Icon /> Clear
                </Button>
              )}
            </>
          )
        }
      />
      <ErrorNote error={log.error ?? actions.setEnabled.error ?? actions.clear.error} />
      {data && !data.enabled && (
        <Empty icon={<ScanSearchIcon />}>
          <p>
            Turn on capturing, then open a page of a Laravel site. RED adds its query listener
            through Laravel&apos;s own package list, without touching your project.
          </p>
        </Empty>
      )}
      {data?.enabled && data.queries.length === 0 && (
        <Empty icon={<ScanSearchIcon />}>
          <p>Waiting for queries… Open a page of a Laravel site.</p>
        </Empty>
      )}
      {data && data.queries.length > 0 && (
        <Input
          aria-label="Filter queries"
          placeholder="Filter by SQL or URL…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="max-w-md"
        />
      )}
      <div className="space-y-4">
        {groups.map((group) => (
          <RequestCard key={group.key} group={group} />
        ))}
      </div>
    </div>
  );
}

function RequestCard({ group }: { group: Group }) {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="truncate font-mono text-sm">{group.request || "Request"}</CardTitle>
        <CardAction className="flex items-center gap-2">
          {group.repeated.size > 0 && (
            <Badge variant="warning" title="The same query ran more than once: an N+1 problem?">
              {group.repeated.size} repeated
            </Badge>
          )}
          <Badge variant="secondary">
            {group.queries.length} {group.queries.length === 1 ? "query" : "queries"}
          </Badge>
          <Badge variant="outline">{group.total.toFixed(1)} ms</Badge>
          <span className="text-xs text-muted-foreground">
            {new Date(group.received).toLocaleTimeString()}
          </span>
        </CardAction>
      </CardHeader>
      <CardContent className="px-4">
        <ol className="divide-y rounded-md border">
          {group.queries.map((query) => (
            <QueryRow key={query.id} query={query} repeated={group.repeated.get(query.sql)} />
          ))}
        </ol>
      </CardContent>
    </Card>
  );
}

function QueryRow({ query, repeated }: { query: QueryEntry; repeated?: number }) {
  const slow = query.time_ms >= SLOW_MS;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(query.bound);
      toast.success("Query copied");
    } catch {
      toast.error("Could not copy to the clipboard");
    }
  };
  return (
    <li className="group flex items-start gap-3 px-3 py-2 text-xs">
      <span
        className={cn(
          "w-16 shrink-0 pt-0.5 text-right font-mono tabular-nums",
          slow ? "font-semibold text-destructive" : "text-muted-foreground",
        )}
      >
        {query.time_ms.toFixed(2)} ms
      </span>
      <div className="min-w-0 flex-1 space-y-1">
        <code className="block font-mono break-words whitespace-pre-wrap">{query.bound}</code>
        <div className="flex flex-wrap items-center gap-2 text-muted-foreground">
          {query.file && (
            <span className="font-mono" title={query.file}>
              {shortPath(query.file)}
              {query.line !== null && `:${query.line}`}
            </span>
          )}
          {query.connection && <span>{query.connection}</span>}
          {repeated && <Badge variant="warning">×{repeated}</Badge>}
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
        aria-label="Copy query"
        onClick={copy}
      >
        <CopyIcon />
      </Button>
    </li>
  );
}
