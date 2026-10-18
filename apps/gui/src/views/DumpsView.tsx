import { useDumpActions, useDumps } from "../api/queries";
import { Badge, Button, Card, ErrorNote, HtmlFrame } from "../components/ui";

export function DumpsView() {
  const dumps = useDumps();
  const actions = useDumpActions();
  const data = dumps.data;
  const entries = [...(data?.dumps ?? [])].reverse();

  return (
    <Card
      title="Dumps"
      actions={
        data && (
          <>
            <Button
              variant={data.enabled ? "secondary" : "primary"}
              aria-pressed={data.enabled}
              onClick={() => actions.setEnabled.mutate(!data.enabled)}
            >
              {data.enabled ? "Capturing — turn off" : "Capture dumps"}
            </Button>
            {entries.length > 0 && (
              <Button variant="ghost" onClick={() => actions.clear.mutate()}>
                Clear
              </Button>
            )}
          </>
        )
      }
    >
      <ErrorNote error={dumps.error ?? actions.setEnabled.error ?? actions.clear.error} />
      {data && !data.enabled && (
        <p className="mb-3 text-sm text-stone-500">
          While capturing, <code>dump()</code> and <code>dd()</code> output from your sites shows up
          here instead of in the page. For artisan commands, set{" "}
          <code>VAR_DUMPER_FORMAT=server</code> in the environment.
        </p>
      )}
      {data?.enabled && entries.length === 0 && (
        <p className="text-sm text-stone-500">
          Waiting for <code>dump()</code> calls…
        </p>
      )}
      <ol className="space-y-4">
        {entries.map((dump) => (
          <li key={dump.id} className="rounded-lg border border-stone-200 dark:border-stone-800">
            <div className="flex flex-wrap items-center gap-2 border-b border-stone-200 px-3 py-1.5 text-xs text-stone-500 dark:border-stone-800">
              <span>{new Date(dump.received).toLocaleTimeString()}</span>
              {dump.source && (
                <span className="font-mono" title={dump.source.file}>
                  {dump.source.name}
                  {dump.source.line !== null && `:${dump.source.line}`}
                </span>
              )}
              {dump.origin && <Badge>{dump.origin}</Badge>}
            </div>
            <HtmlFrame
              title={`Dump ${dump.id}`}
              html={dump.html}
              css="html,body{margin:0;background:#18171B}pre.sf-dump{margin:0;padding:8px 10px}"
            />
          </li>
        ))}
      </ol>
    </Card>
  );
}
