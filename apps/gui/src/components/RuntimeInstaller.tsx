import type { RuntimeJob } from "../api/gen/RuntimeJob";
import type { RuntimeKind } from "../api/gen/RuntimeKind";
import { useRuntimeActions, useRuntimes } from "../api/queries";
import { Badge, Button, ErrorNote } from "./ui";

const label: Record<RuntimeKind, string> = {
  php: "PHP",
  mysql: "MySQL",
  postgres: "PostgreSQL",
  valkey: "Valkey",
  node: "Node.js",
  composer: "Composer",
};

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
  if (!data) return <p className="text-sm text-stone-500">Looking for downloads…</p>;

  const available = data.available.filter((r) => kinds.includes(r.kind));
  const failed = data.jobs.find((j) => kinds.includes(j.kind) && j.state === "failed");

  return (
    <div className="space-y-3 text-sm">
      <ErrorNote error={actions.install.error ?? actions.remove.error} />
      {failed && (
        <p role="alert" className="text-sm text-red-700 dark:text-red-400">
          Installing {label[failed.kind]} {failed.version} failed: {failed.error}
        </p>
      )}
      {available.length === 0 && (
        <p className="text-stone-500">
          Nothing to download for {data.platform}
          {data.catalog_errors.length > 0 && ` (${data.catalog_errors.join("; ")})`}.
        </p>
      )}
      <ul className="divide-y divide-stone-100 dark:divide-stone-800">
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
              className="flex items-center justify-between gap-3 py-2"
            >
              <div className="flex items-center gap-2">
                <span className="font-medium">
                  {label[release.kind]} {release.line}
                </span>
                <span className="text-xs text-stone-500">{release.version}</span>
                {installed && (
                  <Badge tone={upToDate ? "good" : "warn"}>
                    {upToDate ? "installed" : `${installed.version} installed`}
                  </Badge>
                )}
              </div>
              <div className="flex items-center gap-2">
                {job ? (
                  <span className="flex items-center gap-2 text-xs text-stone-500">
                    <span
                      className="h-1.5 w-24 overflow-hidden rounded bg-stone-200 dark:bg-stone-800"
                      aria-hidden
                    >
                      <span
                        className="block h-full bg-red-600 transition-all"
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
                      variant={installed ? "secondary" : "primary"}
                      onClick={() =>
                        actions.install.mutate({ kind: release.kind, version: release.line })
                      }
                    >
                      {!installed
                        ? "Install"
                        : update === installed.version
                          ? "Switch to RED build"
                          : `Update to ${update}`}
                    </Button>
                  )
                )}
                {installed && !job && (
                  <Button
                    variant="ghost"
                    onClick={() =>
                      actions.remove.mutate({ kind: release.kind, line: release.line })
                    }
                  >
                    Remove
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
