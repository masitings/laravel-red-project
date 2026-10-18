import { RedApiError } from "./api/client";
import type { StatusResponse } from "./api/gen/StatusResponse";
import { useStatus } from "./api/status";

export function App() {
  return (
    <div className="min-h-screen bg-stone-50 text-stone-900 dark:bg-stone-950 dark:text-stone-100">
      <header className="flex items-center gap-3 border-b border-stone-200 px-6 py-4 dark:border-stone-800">
        <span className="grid size-8 place-items-center rounded-lg bg-red-600 text-sm font-bold text-white">
          R
        </span>
        <h1 className="text-lg font-semibold">RED</h1>
      </header>
      <main className="mx-auto max-w-2xl p-6">
        <DaemonStatus />
      </main>
    </div>
  );
}

function DaemonStatus() {
  const { data, error, isPending } = useStatus();

  if (isPending) {
    return <p className="text-stone-500">Connecting to redd…</p>;
  }
  if (error) {
    return <Offline error={error} />;
  }
  return <Online status={data} />;
}

function Online({ status }: { status: StatusResponse }) {
  const rows: [string, string][] = [
    ["Version", `${status.version} (API v${status.api_version}, ${status.os})`],
    ["Uptime", formatUptime(status.uptime_secs)],
    ["TLD", `.${status.tld}`],
    ["Config", status.paths.config_dir],
    ["Data", status.paths.data_dir],
    ["Socket", status.paths.socket],
  ];
  return (
    <section className="rounded-xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
      <h2 className="mb-4 flex items-center gap-2 font-medium">
        <span className="size-2.5 rounded-full bg-emerald-500" aria-hidden />
        redd is running
        <span className="text-sm font-normal text-stone-500">pid {status.pid}</span>
      </h2>
      <dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-2 text-sm">
        {rows.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-stone-500">{label}</dt>
            <dd className="break-all font-mono">{value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function Offline({ error }: { error: Error }) {
  const hint = error instanceof RedApiError ? error.hint : undefined;
  return (
    <section
      role="alert"
      className="rounded-xl border border-amber-300 bg-amber-50 p-5 dark:border-amber-800 dark:bg-amber-950"
    >
      <h2 className="mb-2 flex items-center gap-2 font-medium">
        <span className="size-2.5 rounded-full bg-amber-500" aria-hidden />
        redd is not reachable
      </h2>
      <p className="text-sm">{error.message}</p>
      {hint && <p className="mt-2 text-sm text-stone-600 dark:text-stone-400">{hint}</p>}
    </section>
  );
}

export function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}
