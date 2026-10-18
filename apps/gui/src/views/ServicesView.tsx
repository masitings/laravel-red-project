import { type FormEvent, useState } from "react";
import type { ServiceInfo } from "../api/gen/ServiceInfo";
import type { ServiceStatus } from "../api/gen/ServiceStatus";
import { useServiceActions, useServices } from "../api/queries";
import { Badge, Button, Card, ErrorNote } from "../components/ui";

const statusTone: Record<ServiceStatus, "good" | "warn" | "bad" | "neutral"> = {
  running: "good",
  starting: "warn",
  failed: "bad",
  stopped: "neutral",
};

const installHint: Record<string, string> = {
  mysql: "brew install mysql (or mariadb)",
  postgres: "brew install postgresql@17",
  redis: "brew install valkey (or redis)",
};

export function ServicesView() {
  const services = useServices();
  const actions = useServiceActions();
  const failed = Object.values(actions).find((a) => a.error);
  const list = services.data?.services ?? [];

  return (
    <div className="space-y-6">
      <Card title="Services">
        <ErrorNote error={failed?.error ?? services.error} />
        {services.data && list.length === 0 && (
          <p className="text-sm text-stone-500">
            No services yet. Add MySQL, PostgreSQL or Redis below; RED runs them on 127.0.0.1 and
            starts them with redd.
          </p>
        )}
        <ul className="divide-y divide-stone-100 dark:divide-stone-800">
          {list.map((service) => (
            <ServiceRow key={service.name} service={service} />
          ))}
        </ul>
      </Card>
      <AddService />
    </div>
  );
}

function ServiceRow({ service }: { service: ServiceInfo }) {
  const actions = useServiceActions();
  const [confirmRemove, setConfirmRemove] = useState(false);
  const running = service.status === "running" || service.status === "starting";
  const server = [service.flavor ?? service.kind, service.version].filter(Boolean).join(" ");

  return (
    <li className="py-3 text-sm">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 font-medium">
            {service.name}
            <Badge tone={statusTone[service.status]}>{service.status}</Badge>
          </div>
          <div className="text-xs text-stone-500">
            {server} · 127.0.0.1:{service.port}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <label className="flex items-center gap-1 text-xs text-stone-500">
            <input
              type="checkbox"
              checked={service.autostart}
              aria-label={`Start ${service.name} with redd`}
              onChange={(e) =>
                actions.autostart.mutate({ name: service.name, on: e.target.checked })
              }
            />
            autostart
          </label>
          <Button
            onClick={() =>
              actions.action.mutate({ name: service.name, action: running ? "stop" : "start" })
            }
          >
            {running ? "Stop" : "Start"}
          </Button>
          {running && (
            <Button
              variant="ghost"
              onClick={() => actions.action.mutate({ name: service.name, action: "restart" })}
            >
              Restart
            </Button>
          )}
          {confirmRemove ? (
            <>
              <Button
                variant="ghost"
                onClick={() => actions.remove.mutate({ name: service.name, deleteData: false })}
              >
                Remove, keep data
              </Button>
              <Button
                variant="ghost"
                className="text-red-700 dark:text-red-400"
                onClick={() => actions.remove.mutate({ name: service.name, deleteData: true })}
              >
                Remove and delete data
              </Button>
            </>
          ) : (
            <Button variant="ghost" onClick={() => setConfirmRemove(true)}>
              Remove
            </Button>
          )}
        </div>
      </div>
      {service.status === "failed" && service.error && (
        <pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded bg-red-50 p-2 text-xs text-red-800 dark:bg-red-950 dark:text-red-300">
          {service.error}
          {"\n"}Log: {service.log_file}
        </pre>
      )}
      <details className="mt-2">
        <summary className="cursor-pointer text-xs text-stone-500">.env settings</summary>
        <pre className="mt-1 select-all rounded bg-stone-100 p-2 text-xs dark:bg-stone-800">
          {service.env.map((v) => `${v.name}=${v.value}`).join("\n")}
        </pre>
      </details>
    </li>
  );
}

function AddService() {
  const services = useServices();
  const actions = useServiceActions();
  const available = services.data?.available ?? [];
  const [choice, setChoice] = useState("");
  const [port, setPort] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const install = available[Number(choice)];
    if (!install) return;
    actions.add.mutate(
      {
        kind: install.kind,
        version: install.full_version,
        port: port ? Number(port) : undefined,
      },
      { onSuccess: () => setPort("") },
    );
  };

  const kinds = ["mysql", "postgres", "redis"] as const;
  const missing = kinds.filter((k) => !available.some((i) => i.kind === k));

  return (
    <Card title="Add a service">
      {services.data && available.length === 0 ? (
        <p className="text-sm text-stone-500">
          No database servers found. Install one with Homebrew, e.g. <code>brew install mysql</code>
          , and it shows up here.
        </p>
      ) : (
        <form onSubmit={submit} className="flex flex-wrap items-center gap-2">
          <select
            aria-label="Server"
            className="rounded border border-stone-300 bg-transparent px-2 py-1 text-sm dark:border-stone-700"
            value={choice}
            onChange={(e) => setChoice(e.target.value)}
            required
          >
            <option value="">Choose a server…</option>
            {available.map((install, index) => (
              <option key={install.binary} value={index}>
                {install.flavor} {install.full_version}
              </option>
            ))}
          </select>
          <input
            aria-label="Port"
            placeholder="port (default)"
            inputMode="numeric"
            className="w-32 rounded border border-stone-300 bg-transparent px-2 py-1 text-sm dark:border-stone-700"
            value={port}
            onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
          />
          <Button type="submit" variant="primary" disabled={actions.add.isPending}>
            Add service
          </Button>
        </form>
      )}
      {missing.length > 0 && available.length > 0 && (
        <p className="mt-3 text-xs text-stone-500">
          Not installed: {missing.map((k) => `${k} (${installHint[k]})`).join(", ")}
        </p>
      )}
    </Card>
  );
}
