import { useState } from "react";
import { RedApiError } from "./api/client";
import { useDaemonEvents } from "./api/events";
import { useStatus } from "./api/queries";
import { PhpView } from "./views/PhpView";
import { SitesView } from "./views/SitesView";
import { StatusView } from "./views/StatusView";

export { formatUptime } from "./views/StatusView";

const tabs = ["Sites", "PHP", "Status"] as const;
type Tab = (typeof tabs)[number];

export function App() {
  const [tab, setTab] = useState<Tab>("Sites");
  const status = useStatus();
  useDaemonEvents();

  return (
    <div className="flex min-h-screen bg-stone-50 text-stone-900 dark:bg-stone-950 dark:text-stone-100">
      <nav className="flex w-44 shrink-0 flex-col gap-1 border-r border-stone-200 p-3 dark:border-stone-800">
        <div className="mb-4 flex items-center gap-2 px-2">
          <span className="grid size-7 place-items-center rounded-lg bg-red-600 text-xs font-bold text-white">
            R
          </span>
          <span className="font-semibold">RED</span>
        </div>
        {tabs.map((name) => (
          <button
            key={name}
            type="button"
            onClick={() => setTab(name)}
            aria-current={tab === name ? "page" : undefined}
            className={`rounded-md px-2 py-1.5 text-left text-sm ${
              tab === name
                ? "bg-stone-200 font-medium dark:bg-stone-800"
                : "text-stone-600 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-900"
            }`}
          >
            {name}
          </button>
        ))}
      </nav>
      <main className="min-w-0 flex-1 p-6">
        {status.isPending && <p className="text-stone-500">Connecting to redd…</p>}
        {status.error && <Offline error={status.error} />}
        {status.data && tab === "Sites" && <SitesView />}
        {status.data && tab === "PHP" && <PhpView />}
        {status.data && tab === "Status" && <StatusView status={status.data} />}
      </main>
    </div>
  );
}

function Offline({ error }: { error: Error }) {
  const hint = error instanceof RedApiError ? error.hint : undefined;
  return (
    <section
      role="alert"
      className="max-w-xl rounded-xl border border-amber-300 bg-amber-50 p-5 dark:border-amber-800 dark:bg-amber-950"
    >
      <h2 className="mb-2 flex items-center gap-2 font-medium">
        <span className="size-2.5 rounded-full bg-amber-500" aria-hidden />
        redd is not reachable
      </h2>
      <p className="text-sm">{error.message}</p>
      {hint && <p className="mt-2 text-sm text-stone-600 dark:text-stone-400">{hint}</p>}
      <p className="mt-2 text-sm text-stone-600 dark:text-stone-400">
        To start it automatically at login, run <code>red setup</code> once.
      </p>
    </section>
  );
}
