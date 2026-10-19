import { BugIcon, Trash2Icon } from "lucide-react";
import { useDumpActions, useDumps } from "@/api/queries";
import { Empty, ErrorNote, HtmlFrame, PageHeader } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

export function DumpsView() {
  const dumps = useDumps();
  const actions = useDumpActions();
  const data = dumps.data;
  const entries = [...(data?.dumps ?? [])].reverse();

  return (
    <div className="space-y-6">
      <PageHeader
        title="Dumps"
        description={
          <>
            <code>dump()</code> and <code>dd()</code> output from your sites, instead of in the
            page.
          </>
        }
        actions={
          data && (
            <>
              <div className="flex items-center gap-2">
                <Switch
                  id="capture-dumps"
                  checked={data.enabled}
                  onCheckedChange={(on) => actions.setEnabled.mutate(on)}
                />
                <Label htmlFor="capture-dumps">Capture dumps</Label>
              </div>
              {entries.length > 0 && (
                <Button variant="outline" onClick={() => actions.clear.mutate()}>
                  <Trash2Icon /> Clear
                </Button>
              )}
            </>
          )
        }
      />
      <ErrorNote error={dumps.error ?? actions.setEnabled.error ?? actions.clear.error} />
      {data && !data.enabled && (
        <Empty icon={<BugIcon />}>
          <p>
            Turn on capturing to see dumps here. For artisan commands, set{" "}
            <code>VAR_DUMPER_FORMAT=server</code> in the environment.
          </p>
        </Empty>
      )}
      {data?.enabled && entries.length === 0 && (
        <Empty icon={<BugIcon />}>
          <p>
            Waiting for <code>dump()</code> calls…
          </p>
        </Empty>
      )}
      <ol className="space-y-4">
        {entries.map((dump) => (
          <li key={dump.id}>
            <Card className="gap-0 overflow-hidden py-0">
              <div className="flex flex-wrap items-center gap-2 border-b px-3 py-1.5 text-xs text-muted-foreground">
                <span>{new Date(dump.received).toLocaleTimeString()}</span>
                {dump.source && (
                  <span className="font-mono" title={dump.source.file}>
                    {dump.source.name}
                    {dump.source.line !== null && `:${dump.source.line}`}
                  </span>
                )}
                {dump.origin && <Badge variant="secondary">{dump.origin}</Badge>}
              </div>
              <HtmlFrame
                title={`Dump ${dump.id}`}
                html={dump.html}
                css="html,body{margin:0;background:#18171B}pre.sf-dump{margin:0;padding:8px 10px}"
              />
            </Card>
          </li>
        ))}
      </ol>
    </div>
  );
}
