import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { type FormEvent, useEffect, useState } from "react";
import { api, RedApiError } from "../api/client";
import type { SettingsResponse } from "../api/gen/SettingsResponse";
import { keys, useDoctor, useParkedPaths, useSiteActions } from "../api/queries";
import { Button, Card, ErrorNote } from "../components/ui";

const settingsKey = ["settings"] as const;

function toError(err: unknown): unknown {
  if (err instanceof Error) return err;
  if (typeof err === "object" && err !== null && "message" in err) {
    return new RedApiError(err as ConstructorParameters<typeof RedApiError>[0]);
  }
  return new Error(String(err));
}

export function SettingsView() {
  return (
    <div className="space-y-6">
      {isTauri() && <Startup />}
      <Folders />
      <Ports />
    </div>
  );
}

function Startup() {
  const client = useQueryClient();
  const doctor = useDoctor();
  const autostart = doctor.data?.checks.find((c) => c.id === "autostart");
  const app = useQuery({
    queryKey: ["app-autostart"],
    queryFn: () => invoke<boolean>("app_autostart"),
  });
  const setApp = useMutation({
    mutationFn: async (on: boolean) => {
      try {
        return await invoke<boolean>("set_app_autostart", { on });
      } catch (err) {
        throw toError(err);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: ["app-autostart"] }),
  });
  const daemon = useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("setup_step", { step: "autostart" });
      } catch (err) {
        throw toError(err);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: keys.doctor }),
  });
  return (
    <Card title="Startup">
      <div className="space-y-4 text-sm">
        <label className="flex items-start gap-3">
          <input
            type="checkbox"
            className="mt-1"
            checked={app.data ?? false}
            disabled={app.isPending || setApp.isPending}
            onChange={(e) => setApp.mutate(e.target.checked)}
          />
          <span>
            <span className="block font-medium">Open RED at login</span>
            <span className="block text-stone-500">
              The app starts in the menu bar, with your sites and services one click away.
            </span>
          </span>
        </label>
        <ErrorNote error={setApp.error ?? app.error} />
        <div className="flex items-start justify-between gap-3">
          <div>
            <span className="block font-medium">Run redd at login</span>
            <span className="block text-stone-500">
              {autostart?.status === "ok"
                ? "redd starts at login, so sites work even when the app is closed."
                : "redd only runs while started by hand or by the app."}
            </span>
          </div>
          {autostart && autostart.status !== "ok" && (
            <Button onClick={() => daemon.mutate()} disabled={daemon.isPending}>
              {daemon.isPending ? "Setting up…" : "Start at login"}
            </Button>
          )}
        </div>
        <ErrorNote error={daemon.error} />
      </div>
    </Card>
  );
}

function Folders() {
  const paths = useParkedPaths();
  const actions = useSiteActions();
  const client = useQueryClient();
  const forget = useMutation({
    mutationFn: (path: string) => api("DELETE", "/v1/paths", { path }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: keys.paths });
      client.invalidateQueries({ queryKey: keys.sites });
    },
  });
  const add = async () => {
    const picked = await pickFolder({ directory: true, multiple: false });
    if (typeof picked === "string") actions.park.mutate(picked);
  };
  return (
    <Card title="Parked folders" actions={<Button onClick={add}>Add folder…</Button>}>
      <p className="mb-3 text-sm text-stone-500">
        Every folder inside these is a site. New sites are created in the first one.
      </p>
      <ErrorNote error={paths.error ?? forget.error ?? actions.park.error} />
      {paths.data?.length === 0 && <p className="text-sm text-stone-500">No parked folders.</p>}
      <ul className="divide-y divide-stone-100 text-sm dark:divide-stone-800">
        {paths.data?.map((path) => (
          <li key={path} className="flex items-center justify-between gap-3 py-2">
            <code className="min-w-0 truncate">{path}</code>
            <Button
              variant="ghost"
              onClick={() => forget.mutate(path)}
              disabled={forget.isPending}
              aria-label={`Stop serving ${path}`}
            >
              Remove
            </Button>
          </li>
        ))}
      </ul>
    </Card>
  );
}

type PortField = "smtp_port" | "dump_port" | "xdebug_port";

const portFields: { key: PortField; label: string; help: string }[] = [
  { key: "smtp_port", label: "Mail (SMTP)", help: "Where apps send mail; RED catches it." },
  { key: "dump_port", label: "Dumps", help: "Where dump() output goes when captured." },
  { key: "xdebug_port", label: "Xdebug", help: "Where your IDE listens for Xdebug." },
];

function Ports() {
  const client = useQueryClient();
  const settings = useQuery({
    queryKey: settingsKey,
    queryFn: () => api<SettingsResponse>("GET", "/v1/settings"),
  });
  const [form, setForm] = useState<Record<PortField, string> & { auto: boolean }>({
    smtp_port: "",
    dump_port: "",
    xdebug_port: "",
    auto: false,
  });
  const data = settings.data;
  useEffect(() => {
    if (data) {
      setForm({
        smtp_port: String(data.smtp_port),
        dump_port: String(data.dump_port),
        xdebug_port: String(data.xdebug_port),
        auto: data.runtimes_auto_update,
      });
    }
  }, [data]);
  const save = useMutation({
    mutationFn: () =>
      api<SettingsResponse>("PUT", "/v1/settings", {
        smtp_port: Number(form.smtp_port),
        dump_port: Number(form.dump_port),
        xdebug_port: Number(form.xdebug_port),
        runtimes_auto_update: form.auto,
      }),
    onSuccess: (next) => client.setQueryData(settingsKey, next),
  });
  const restart = useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("restart_daemon");
      } catch (err) {
        throw toError(err);
      }
    },
    onSuccess: () => client.invalidateQueries(),
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };
  if (!data) return <ErrorNote error={settings.error} />;
  return (
    <Card title="Ports and updates">
      <form onSubmit={submit} className="space-y-4 text-sm">
        {data.restart_required && (
          <div
            role="status"
            className="flex items-center justify-between gap-3 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 dark:border-amber-800 dark:bg-amber-950"
          >
            <span>Saved. Restart redd to use the new settings.</span>
            {isTauri() && (
              <Button onClick={() => restart.mutate()} disabled={restart.isPending}>
                {restart.isPending ? "Restarting…" : "Restart redd"}
              </Button>
            )}
          </div>
        )}
        <ErrorNote error={restart.error} />
        <div className="grid gap-3 sm:grid-cols-3">
          {portFields.map(({ key, label, help }) => (
            <label key={key} className="block space-y-1">
              <span className="font-medium">{label}</span>
              <input
                type="number"
                min={1024}
                max={65535}
                required
                value={form[key]}
                onChange={(e) => setForm({ ...form, [key]: e.target.value })}
                aria-label={`${label} port`}
                className="w-full rounded-md border border-stone-300 bg-white px-2 py-1 font-mono dark:border-stone-700 dark:bg-stone-950"
              />
              <span className="block text-xs text-stone-500">{help}</span>
            </label>
          ))}
        </div>
        <label className="flex items-start gap-3">
          <input
            type="checkbox"
            className="mt-1"
            checked={form.auto}
            onChange={(e) => setForm({ ...form, auto: e.target.checked })}
          />
          <span>
            <span className="block font-medium">Install runtime updates automatically</span>
            <span className="block text-stone-500">
              New patch releases of PHP, databases, Node.js and Composer that RED installed.
            </span>
          </span>
        </label>
        <ErrorNote error={save.error} />
        <div className="flex items-center justify-between gap-3">
          <p className="text-xs text-stone-500">
            Sites use <code>.{data.tld}</code> on ports {data.http_port} and {data.https_port}.
            These live in <code>{data.config_file}</code>.
          </p>
          <Button type="submit" variant="primary" disabled={save.isPending}>
            Save
          </Button>
        </div>
      </form>
    </Card>
  );
}
