import { DownloadIcon, Trash2Icon } from "lucide-react";
import type { RuntimeJob } from "@/api/gen/RuntimeJob";
import type { RuntimeKind } from "@/api/gen/RuntimeKind";
import { useRuntimeActions, useRuntimes } from "@/api/queries";
import { ErrorNote } from "@/components/common";
import { runtimeLabel } from "@/components/runtime-labels";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

function progress(job: RuntimeJob): string {
  if (job.state === "extracting") return "Unpacking…";
  if (job.total) return `${Math.round((job.received / job.total) * 100)}%`;
  return `${(job.received / 1_048_576).toFixed(1)} MB`;
}

/**
 * Versions of `kinds` RED can download, with install, update and remove
 * buttons and live progress.
 */
export function RuntimeInstaller({ kinds }: { kinds: RuntimeKind[] }) {
  const runtimes = useRuntimes();
  const actions = useRuntimeActions();
  const data = runtimes.data;
  if (runtimes.error) return <ErrorNote error={runtimes.error} />;
  if (!data) return <p className="text-sm text-muted-foreground">Looking for downloads…</p>;

  const available = data.available.filter((r) => kinds.includes(r.kind));
  const failed = data.jobs.find((j) => kinds.includes(j.kind) && j.state === "failed");

  return (
    <div className="space-y-3 text-sm">
      <ErrorNote error={actions.install.error ?? actions.remove.error} />
      {failed && (
        <ErrorNote
          error={
            new Error(
              `Installing ${runtimeLabel[failed.kind]} ${failed.version} failed: ${failed.error}`,
            )
          }
        />
      )}
      {available.length === 0 && (
        <p className="text-muted-foreground">
          Nothing to download for {data.platform}
          {data.catalog_errors.length > 0 && ` (${data.catalog_errors.join("; ")})`}.
        </p>
      )}
      <ul className={available.length === 0 ? "hidden" : "divide-y rounded-md border"}>
        {available.map((release) => {
          const installed = data.installed.find(
            (i) => i.kind === release.kind && i.line === release.line,
          );
          const job = data.jobs.find(
            (j) =>
              j.kind === release.kind &&
              j.line === release.line &&
              (j.state === "downloading" || j.state === "extracting"),
          );
          // The daemon says what to update to; RED's own PHP build of the
          // same version counts too (it has more extensions).
          const update = installed?.update ?? null;
          const upToDate = installed !== undefined && update === null;
          return (
            <li
              key={`${release.kind}-${release.line}`}
              className="flex items-center justify-between gap-3 px-3 py-2"
            >
              <div className="flex min-w-0 items-center gap-2">
                <span className="font-medium">
                  {runtimeLabel[release.kind]} {release.line}
                </span>
                <span className="font-mono text-xs text-muted-foreground">{release.version}</span>
                {installed && (
                  <Badge variant={upToDate ? "success" : "warning"}>
                    {upToDate ? "installed" : `${installed.version} installed`}
                  </Badge>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-2">
                {job ? (
                  <span className="flex items-center gap-2 text-xs text-muted-foreground">
                    <span className="h-1.5 w-24 overflow-hidden rounded-full bg-muted" aria-hidden>
                      <span
                        className="block h-full bg-primary transition-all"
                        style={{
                          width: job.total
                            ? `${Math.round((job.received / job.total) * 100)}%`
                            : "30%",
                        }}
                      />
                    </span>
                    {progress(job)}
                  </span>
                ) : (
                  !upToDate && (
                    <Button
                      size="sm"
                      variant={installed ? "secondary" : "default"}
                      onClick={() =>
                        actions.install.mutate({ kind: release.kind, version: release.line })
                      }
                    >
                      <DownloadIcon />
                      {!installed
                        ? "Install"
                        : update === installed.version
                          ? "Get new build"
                          : `Update to ${update}`}
                    </Button>
                  )
                )}
                {installed && !job && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Remove ${runtimeLabel[release.kind]} ${release.line}`}
                    onClick={() =>
                      actions.remove.mutate({ kind: release.kind, line: release.line })
                    }
                  >
                    <Trash2Icon /> Remove
                  </Button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
