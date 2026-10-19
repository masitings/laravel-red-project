import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  CheckCircle2Icon,
  CircleAlertIcon,
  RefreshCwIcon,
  TerminalIcon,
  WandSparklesIcon,
  XCircleIcon,
} from "lucide-react";
import { toast } from "sonner";
import { api } from "@/api/client";
import type { Check } from "@/api/gen/Check";
import type { FixAction } from "@/api/gen/FixAction";
import type { ServicesResponse } from "@/api/gen/ServicesResponse";
import type { StatusResponse } from "@/api/gen/StatusResponse";
import { keys, useDoctor } from "@/api/queries";
import { ErrorNote, PageHeader, toApiError } from "@/components/common";
import { UpdatesCard } from "@/components/Updates";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

/** Fixes the app can run itself (the rest need the CLI). */
async function runFix(action: FixAction): Promise<void> {
  const step = async (name: string) => {
    try {
      await invoke<string>("setup_step", { step: name });
    } catch (err) {
      throw toApiError(err);
    }
  };
  switch (action) {
    case "install_helper":
      return step("system");
    case "start_at_login":
      return step("autostart");
    case "trust_ca":
      return step("trust");
    case "install_dns":
      await api("POST", "/v1/dns");
      return;
    case "install_php":
      await api("POST", "/v1/runtimes", { kind: "php", version: "" });
      return;
    case "restart_services": {
      const services = await api<ServicesResponse>("GET", "/v1/services");
      for (const s of services.services.filter((s) => s.status === "failed")) {
        await api("POST", `/v1/services/${encodeURIComponent(s.name)}/restart`);
      }
      return;
    }
  }
}

/** Steps that need the Tauri shell (password dialogs, the login agent). */
const needsShell: FixAction[] = ["install_helper", "start_at_login", "trust_ca"];

const fixable = (check: Check) =>
  check.action !== undefined && (isTauri() || !needsShell.includes(check.action));

/** Run fixes in an order that works: helper before restarting redd. */
const order: FixAction[] = [
  "install_helper",
  "start_at_login",
  "install_php",
  "install_dns",
  "trust_ca",
  "restart_services",
];

export function StatusView({ status, onSetup }: { status: StatusResponse; onSetup?: () => void }) {
  const doctor = useDoctor();
  const client = useQueryClient();
  const checks = doctor.data?.checks ?? [];
  const fixes = checks.filter(fixable);
  const fix = useMutation({
    mutationFn: async (actions: FixAction[]) => {
      for (const action of order.filter((a) => actions.includes(a))) {
        await runFix(action);
      }
    },
    onSuccess: () => toast.success("Fixed"),
    onError: (err) => {
      if (err instanceof Error && "code" in err && err.code === "cancelled") return;
      toast.error("Could not fix it", { description: err.message });
    },
    onSettled: () => {
      client.invalidateQueries({ queryKey: keys.doctor });
      client.invalidateQueries({ queryKey: keys.status });
    },
  });
  const rows: [string, string][] = [
    ["Version", `${status.version} (API v${status.api_version}, ${status.os})`],
    ["Uptime", formatUptime(status.uptime_secs)],
    ["HTTP", address("http", status.http_port, 80, status.tld)],
    ["HTTPS", address("https", status.https_port, 443, status.tld)],
    ["Mail", status.smtp_port === null ? "not running" : `smtp://127.0.0.1:${status.smtp_port}`],
    ["Dumps", status.dump_port === null ? "not running" : `127.0.0.1:${status.dump_port}`],
    ["Config", status.paths.config_dir],
    ["Data", status.paths.data_dir],
  ];
  return (
    <div className="space-y-6">
      <PageHeader
        title="Status"
        description={`redd is running · pid ${status.pid}`}
        actions={
          isTauri() &&
          onSetup && (
            <Button variant="outline" onClick={onSetup}>
              Set up RED…
            </Button>
          )
        }
      />
      <Card>
        <CardHeader>
          <CardTitle>Doctor</CardTitle>
          <CardDescription>
            Everything RED needs, checked; most problems fix themselves.
          </CardDescription>
          <CardAction className="flex gap-2">
            {fixes.length > 0 && (
              <Button
                size="sm"
                onClick={() => fix.mutate(fixes.map((c) => c.action as FixAction))}
                disabled={fix.isPending}
              >
                <WandSparklesIcon /> {fix.isPending ? "Fixing…" : "Fix all"}
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              onClick={() => doctor.refetch()}
              disabled={doctor.isFetching}
            >
              <RefreshCwIcon /> Check again
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent>
          <ErrorNote error={doctor.error} />
          <ul className="divide-y rounded-md border text-sm">
            {checks.map((check) => (
              <li key={check.id} className="flex items-start gap-3 px-3 py-2.5">
                {check.status === "ok" ? (
                  <CheckCircle2Icon
                    className="mt-0.5 size-4 shrink-0 text-success"
                    aria-label="ok"
                  />
                ) : check.status === "warn" ? (
                  <CircleAlertIcon
                    className="mt-0.5 size-4 shrink-0 text-warning"
                    aria-label="warn"
                  />
                ) : (
                  <XCircleIcon
                    className="mt-0.5 size-4 shrink-0 text-destructive"
                    aria-label="fail"
                  />
                )}
                <div className="min-w-0 flex-1">
                  <span className="font-medium">{check.label}</span>{" "}
                  <span className="text-muted-foreground">{check.detail}</span>
                  {check.status !== "ok" && check.fix && (
                    <div className="text-xs text-muted-foreground">
                      Fix: <code>{check.fix}</code>
                    </div>
                  )}
                </div>
                {check.status !== "ok" && fixable(check) && (
                  <Button
                    size="xs"
                    variant="outline"
                    disabled={fix.isPending}
                    onClick={() => fix.mutate([check.action as FixAction])}
                  >
                    Fix
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>redd</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-2 text-sm">
            {rows.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted-foreground">{label}</dt>
                <dd className="font-mono break-all">{value}</dd>
              </div>
            ))}
          </dl>
        </CardContent>
      </Card>
      <UpdatesCard status={status} />
      {isTauri() && <CliCard />}
    </div>
  );
}

type CliStatus = { bundled: string | null; installed: string | null };

function CliCard() {
  const cli = useQuery({
    queryKey: ["cli"],
    queryFn: () => invoke<CliStatus>("cli_status"),
  });
  const install = useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("install_cli");
      } catch (err) {
        throw toApiError(err);
      }
    },
    onSettled: () => cli.refetch(),
  });
  if (!cli.data) return null;
  const { bundled, installed } = cli.data;
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <TerminalIcon className="size-4" /> Command-line tool
        </CardTitle>
        <CardDescription>
          {installed ? (
            <>
              <code>red</code> is installed at <code>{installed}</code>.
            </>
          ) : (
            <>
              <code>red</code> is not on your PATH yet.
            </>
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        {bundled && !installed && (
          <Button onClick={() => install.mutate()} disabled={install.isPending}>
            Install the red command
          </Button>
        )}
        {install.data && (
          <p className="text-xs text-muted-foreground">
            Linked {install.data}. Make sure <code>~/.local/bin</code> is on your PATH.
          </p>
        )}
        <ErrorNote error={install.error} />
      </CardContent>
    </Card>
  );
}

function address(scheme: string, port: number | null, standard: number, tld: string): string {
  if (port === null) return "not running";
  return `${scheme}://*.${tld}${port === standard ? "" : `:${port}`}`;
}

export function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}
