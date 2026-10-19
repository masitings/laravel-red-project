import { useQueryClient } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useState } from "react";
import { RedApiError } from "./api/client";
import { useDaemonEvents, useNavigateEvents } from "./api/events";
import { keys, useDoctor, useMail, useStatus } from "./api/queries";
import { useUpdateCount } from "./components/Updates";
import { Button, ErrorNote } from "./components/ui";
import { DumpsView } from "./views/DumpsView";
import { LogsView } from "./views/LogsView";
import { MailView } from "./views/MailView";
import { PhpView } from "./views/PhpView";
import { ServicesView } from "./views/ServicesView";
import { SettingsView } from "./views/SettingsView";
import { needsSetup, SetupView, setupDismissed } from "./views/SetupView";
import { SitesView } from "./views/SitesView";
import { StatusView } from "./views/StatusView";

export { formatUptime } from "./views/StatusView";

const tabs = ["Sites", "PHP", "Services", "Mail", "Dumps", "Logs", "Status", "Settings"] as const;
type Tab = (typeof tabs)[number];

export function App() {
  const [tab, setTab] = useState<Tab>("Sites");
  // `null`: show setup by itself on a machine that is not set up yet.
  const [setupOpen, setSetupOpen] = useState<boolean | null>(null);
  const status = useStatus();
  useDaemonEvents();
  useNavigateEvents(
    useCallback((name: string) => {
      const found = tabs.find((t) => t === name);
      if (found) {
        setTab(found);
        setSetupOpen(false);
      }
    }, []),
  );
  const mail = useMail();
  const unread = status.data ? (mail.data?.filter((m) => !m.read).length ?? 0) : 0;
  const updates = useUpdateCount();
  const doctor = useDoctor();
  const showSetup =
    status.data !== undefined &&
    (setupOpen ?? (isTauri() && !setupDismissed() && needsSetup(doctor.data?.checks)));

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
            onClick={() => {
              setTab(name);
              setSetupOpen(false);
            }}
            aria-current={tab === name && !showSetup ? "page" : undefined}
            className={`rounded-md px-2 py-1.5 text-left text-sm ${
              tab === name && !showSetup
                ? "bg-stone-200 font-medium dark:bg-stone-800"
                : "text-stone-600 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-900"
            }`}
          >
            <span className="flex items-center justify-between">
              {name}
              {name === "Status" && status.data && updates > 0 && (
                <span className="rounded-full bg-amber-500 px-1.5 text-xs font-medium text-white">
                  {updates}
                  <span className="sr-only"> updates available</span>
                </span>
              )}
              {name === "Mail" && unread > 0 && (
                <span className="rounded-full bg-red-600 px-1.5 text-xs font-medium text-white">
                  {unread}
                  <span className="sr-only"> unread</span>
                </span>
              )}
            </span>
          </button>
        ))}
      </nav>
      <main className="min-w-0 flex-1 p-6">
        {status.isPending && <p className="text-stone-500">Connecting to redd…</p>}
        {status.error && <Offline error={status.error} />}
        {status.data && showSetup && (
          <SetupView status={status.data} onClose={() => setSetupOpen(false)} />
        )}
        {status.data && !showSetup && (
          <>
            {tab === "Sites" && <SitesView />}
            {tab === "PHP" && <PhpView />}
            {tab === "Services" && <ServicesView />}
            {tab === "Mail" && <MailView />}
            {tab === "Dumps" && <DumpsView />}
            {tab === "Logs" && <LogsView />}
            {tab === "Settings" && <SettingsView />}
            {tab === "Status" && (
              <StatusView status={status.data} onSetup={() => setSetupOpen(true)} />
            )}
          </>
        )}
      </main>
    </div>
  );
}

function Offline({ error }: { error: Error }) {
  const hint = error instanceof RedApiError ? error.hint : undefined;
  const client = useQueryClient();
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<unknown>(null);
  const start = async () => {
    setStarting(true);
    setStartError(null);
    try {
      await invoke<string>("start_daemon");
      // Give redd a moment to open its socket, then look again.
      await new Promise((resolve) => setTimeout(resolve, 1000));
      await client.invalidateQueries({ queryKey: keys.status });
    } catch (err) {
      setStartError(
        err instanceof Error
          ? err
          : new RedApiError(err as ConstructorParameters<typeof RedApiError>[0]),
      );
    } finally {
      setStarting(false);
    }
  };
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
        {isTauri()
          ? "Once it runs, setup offers to start it at login."
          : "To start it automatically at login, run `red setup` once."}
      </p>
      {isTauri() && (
        <div className="mt-4 space-y-2">
          <Button variant="primary" onClick={start} disabled={starting}>
            {starting ? "Starting…" : "Start RED now"}
          </Button>
          <ErrorNote error={startError} />
        </div>
      )}
    </section>
  );
}
