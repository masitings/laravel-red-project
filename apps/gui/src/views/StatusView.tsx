import { useMutation, useQuery } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CheckStatus } from "../api/gen/CheckStatus";
import type { StatusResponse } from "../api/gen/StatusResponse";
import { useDoctor } from "../api/queries";
import { UpdatesCard } from "../components/Updates";
import { Badge, Button, Card, ErrorNote } from "../components/ui";

const tone: Record<CheckStatus, "good" | "warn" | "bad"> = {
  ok: "good",
  warn: "warn",
  fail: "bad",
};

export function StatusView({ status, onSetup }: { status: StatusResponse; onSetup?: () => void }) {
  const doctor = useDoctor();
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
      <Card title={`redd is running · pid ${status.pid}`}>
        <dl className="grid grid-cols-[6rem_1fr] gap-x-4 gap-y-2 text-sm">
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-stone-500">{label}</dt>
              <dd className="break-all font-mono">{value}</dd>
            </div>
          ))}
        </dl>
      </Card>
      <UpdatesCard status={status} />
      {isTauri() && <CliCard />}
      <Card
        title="Doctor"
        actions={
          <>
            {isTauri() && onSetup && <Button onClick={onSetup}>Set up RED…</Button>}
            <Button onClick={() => doctor.refetch()} disabled={doctor.isFetching}>
              Check again
            </Button>
          </>
        }
      >
        <ul className="space-y-2 text-sm">
          {doctor.data?.checks.map((check) => (
            <li key={check.id} className="flex items-start gap-3">
              <Badge tone={tone[check.status]}>{check.status}</Badge>
              <div>
                <span className="font-medium">{check.label}</span>{" "}
                <span className="text-stone-600 dark:text-stone-400">{check.detail}</span>
                {check.status !== "ok" && check.fix && (
                  <div className="text-xs text-stone-500">
                    Fix: <code>{check.fix}</code>
                  </div>
                )}
              </div>
            </li>
          ))}
        </ul>
      </Card>
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
    mutationFn: () => invoke<string>("install_cli"),
    onSuccess: () => cli.refetch(),
  });
  if (!cli.data) return null;
  const { bundled, installed } = cli.data;
  return (
    <Card title="Command-line tool">
      <div className="space-y-2 text-sm">
        {installed ? (
          <p>
            <code>red</code> is installed at <code>{installed}</code>.
          </p>
        ) : (
          <p className="text-stone-600 dark:text-stone-400">
            <code>red</code> is not on your PATH yet.
          </p>
        )}
        {bundled && !installed && (
          <Button variant="primary" onClick={() => install.mutate()} disabled={install.isPending}>
            Install the red command
          </Button>
        )}
        {install.data && (
          <p className="text-xs text-stone-500">
            Linked {install.data}. Make sure <code>~/.local/bin</code> is on your PATH.
          </p>
        )}
        <ErrorNote error={install.error} />
        <p className="text-xs text-stone-500">
          Ports 80/443, DNS and trusted HTTPS: <strong>Set up RED…</strong> below, or{" "}
          <code>red setup</code> in a terminal.
        </p>
      </div>
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
