import { useQueryClient } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  BugIcon,
  CodeIcon,
  DatabaseIcon,
  GlobeIcon,
  HeartPulseIcon,
  type LucideIcon,
  MailIcon,
  ScanSearchIcon,
  ScrollTextIcon,
  ServerIcon,
  SettingsIcon,
  UnplugIcon,
} from "lucide-react";
import { useCallback, useState } from "react";
import { useDaemonEvents, useNavigateEvents } from "@/api/events";
import { keys, useDoctor, useMail, useStatus } from "@/api/queries";
import { ErrorNote, toApiError } from "@/components/common";
import { useUpdateCount } from "@/components/Updates";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { DatabasesView } from "@/views/DatabasesView";
import { DumpsView } from "@/views/DumpsView";
import { LogsView } from "@/views/LogsView";
import { MailView } from "@/views/MailView";
import { PhpView } from "@/views/PhpView";
import { QueriesView } from "@/views/QueriesView";
import { ServicesView } from "@/views/ServicesView";
import { SettingsView } from "@/views/SettingsView";
import { needsSetup, SetupView, setupDismissed } from "@/views/SetupView";
import { SitesView } from "@/views/SitesView";
import { StatusView } from "@/views/StatusView";

export { formatUptime } from "@/views/StatusView";

const tabs = [
  "Sites",
  "PHP",
  "Services",
  "Databases",
  "Queries",
  "Mail",
  "Dumps",
  "Logs",
  "Status",
  "Settings",
] as const;
type Tab = (typeof tabs)[number];

const icons: Record<Tab, LucideIcon> = {
  Sites: GlobeIcon,
  PHP: CodeIcon,
  Services: ServerIcon,
  Databases: DatabaseIcon,
  Queries: ScanSearchIcon,
  Mail: MailIcon,
  Dumps: BugIcon,
  Logs: ScrollTextIcon,
  Status: HeartPulseIcon,
  Settings: SettingsIcon,
};

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
  const go = (name: Tab) => {
    setTab(name);
    setSetupOpen(false);
  };

  return (
    <TooltipProvider delayDuration={300}>
      <div className="flex min-h-screen bg-background text-foreground">
        <nav
          aria-label="Sections"
          className="flex w-52 shrink-0 flex-col gap-0.5 border-r border-sidebar-border bg-sidebar p-3 text-sidebar-foreground"
        >
          <div className="mb-4 flex items-center gap-2 px-2 pt-1">
            <span className="grid size-7 place-items-center rounded-lg bg-primary text-xs font-bold text-primary-foreground">
              R
            </span>
            <span className="font-semibold tracking-tight">RED</span>
            {status.data && (
              <span className="ml-auto font-mono text-[10px] text-muted-foreground">
                {status.data.version}
              </span>
            )}
          </div>
          {tabs.map((name) => {
            const Icon = icons[name];
            const active = tab === name && !showSetup;
            return (
              <button
                key={name}
                type="button"
                onClick={() => go(name)}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors",
                  active
                    ? "bg-sidebar-accent font-medium text-sidebar-accent-foreground"
                    : "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground",
                )}
              >
                <Icon className="size-4 shrink-0" aria-hidden />
                <span className="flex-1">{name}</span>
                {name === "Status" && status.data && updates > 0 && (
                  <span className="rounded-full bg-warning px-1.5 text-[11px] font-medium text-white">
                    {updates}
                    <span className="sr-only"> updates available</span>
                  </span>
                )}
                {name === "Mail" && unread > 0 && (
                  <span className="rounded-full bg-primary px-1.5 text-[11px] font-medium text-primary-foreground">
                    {unread}
                    <span className="sr-only"> unread</span>
                  </span>
                )}
              </button>
            );
          })}
        </nav>
        <main className="min-w-0 flex-1 overflow-x-hidden p-6 lg:p-8">
          <div className="mx-auto max-w-5xl">
            {status.isPending && <p className="text-muted-foreground">Connecting to redd…</p>}
            {status.error && <Offline error={status.error} />}
            {status.data && showSetup && (
              <SetupView status={status.data} onClose={() => setSetupOpen(false)} />
            )}
            {status.data && !showSetup && (
              <>
                {tab === "Sites" && <SitesView />}
                {tab === "PHP" && <PhpView />}
                {tab === "Services" && <ServicesView />}
                {tab === "Databases" && <DatabasesView />}
                {tab === "Queries" && <QueriesView />}
                {tab === "Mail" && <MailView />}
                {tab === "Dumps" && <DumpsView />}
                {tab === "Logs" && <LogsView />}
                {tab === "Status" && (
                  <StatusView status={status.data} onSetup={() => setSetupOpen(true)} />
                )}
                {tab === "Settings" && <SettingsView />}
              </>
            )}
          </div>
        </main>
      </div>
      <Toaster position="bottom-right" />
    </TooltipProvider>
  );
}

function Offline({ error }: { error: Error }) {
  const hint = "hint" in error ? (error.hint as string | undefined) : undefined;
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
      setStartError(toApiError(err));
    } finally {
      setStarting(false);
    }
  };
  return (
    <Card role="alert" className="max-w-xl border-warning/40">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <UnplugIcon className="size-5 text-warning" aria-hidden />
          redd is not reachable
        </CardTitle>
        <CardDescription>{error.message}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3 text-sm text-muted-foreground">
        {hint && <p>{hint}</p>}
        <p>
          {isTauri()
            ? "Once it runs, setup offers to start it at login."
            : "To start it automatically at login, run `red setup` once."}
        </p>
        {isTauri() && (
          <div className="space-y-3">
            <Button onClick={start} disabled={starting}>
              {starting ? "Starting…" : "Start RED now"}
            </Button>
            <ErrorNote error={startError} />
          </div>
        )}
      </CardContent>
    </Card>
  );
}
