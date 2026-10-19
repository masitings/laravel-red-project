import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { FolderPlusIcon, Trash2Icon } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/api/client";
import type { SettingsResponse } from "@/api/gen/SettingsResponse";
import { keys, useDoctor, useParkedPaths, useSiteActions } from "@/api/queries";
import { ErrorNote, PageHeader, toApiError } from "@/components/common";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

const settingsKey = ["settings"] as const;

export function SettingsView() {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Settings"
        description="How RED starts, where it looks for sites, and its ports."
      />
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
        throw toApiError(err);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: ["app-autostart"] }),
  });
  const daemon = useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("setup_step", { step: "autostart" });
      } catch (err) {
        throw toApiError(err);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: keys.doctor }),
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle>Startup</CardTitle>
      </CardHeader>
      <CardContent className="space-y-5 text-sm">
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-0.5">
            <Label htmlFor="open-at-login">Open RED at login</Label>
            <p className="text-muted-foreground">
              The app starts in the menu bar, with your sites and services one click away.
            </p>
          </div>
          <Switch
            id="open-at-login"
            checked={app.data ?? false}
            disabled={app.isPending || setApp.isPending}
            onCheckedChange={(on) => setApp.mutate(on)}
          />
        </div>
        <ErrorNote error={setApp.error ?? app.error} />
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-0.5">
            <span className="text-sm font-medium">Run redd at login</span>
            <p className="text-muted-foreground">
              {autostart?.status === "ok"
                ? "redd starts at login, so sites work even when the app is closed."
                : "redd only runs while started by hand or by the app."}
            </p>
          </div>
          {autostart && autostart.status !== "ok" && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => daemon.mutate()}
              disabled={daemon.isPending}
            >
              {daemon.isPending ? "Setting up…" : "Start at login"}
            </Button>
          )}
        </div>
        <ErrorNote error={daemon.error} />
      </CardContent>
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
    <Card>
      <CardHeader>
        <CardTitle>Parked folders</CardTitle>
        <CardDescription>
          Every folder inside these is a site. New sites are created in the first one.
        </CardDescription>
        <CardAction>
          <Button size="sm" variant="outline" onClick={add}>
            <FolderPlusIcon /> Add folder…
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent className="space-y-3">
        <ErrorNote error={paths.error ?? forget.error ?? actions.park.error} />
        {paths.data?.length === 0 && (
          <p className="text-sm text-muted-foreground">No parked folders.</p>
        )}
        {paths.data && paths.data.length > 0 && (
          <ul className="divide-y rounded-md border text-sm">
            {paths.data.map((path) => (
              <li key={path} className="flex items-center justify-between gap-3 px-3 py-2">
                <code className="min-w-0 truncate">{path}</code>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => forget.mutate(path)}
                  disabled={forget.isPending}
                  aria-label={`Stop serving ${path}`}
                >
                  <Trash2Icon /> Remove
                </Button>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

type PortField = "smtp_port" | "dump_port" | "xdebug_port";

const portFields: { key: PortField; label: string; help: string }[] = [
  { key: "smtp_port", label: "Mail (SMTP)", help: "Where apps send mail; RED catches it." },
  { key: "dump_port", label: "Dumps", help: "Where dump() output and queries go." },
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
        throw toApiError(err);
      }
    },
    onSuccess: () => {
      toast.success("redd restarted");
      client.invalidateQueries();
    },
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };
  if (!data) return <ErrorNote error={settings.error} />;
  return (
    <Card>
      <CardHeader>
        <CardTitle>Ports and updates</CardTitle>
        <CardDescription>
          Sites use <code>.{data.tld}</code> on ports {data.http_port} and {data.https_port}. These
          live in <code>{data.config_file}</code>.
        </CardDescription>
      </CardHeader>
      <form onSubmit={submit}>
        <CardContent className="space-y-5 text-sm">
          {data.restart_required && (
            <Alert className="border-warning/40">
              <AlertDescription className="flex items-center justify-between gap-3">
                <span>Saved. Restart redd to use the new settings.</span>
                {isTauri() && (
                  <Button size="sm" onClick={() => restart.mutate()} disabled={restart.isPending}>
                    {restart.isPending ? "Restarting…" : "Restart redd"}
                  </Button>
                )}
              </AlertDescription>
            </Alert>
          )}
          <ErrorNote error={restart.error} />
          <div className="grid gap-4 sm:grid-cols-3">
            {portFields.map(({ key, label, help }) => (
              <div key={key} className="space-y-1.5">
                <Label htmlFor={`setting-${key}`}>{label}</Label>
                <Input
                  id={`setting-${key}`}
                  type="number"
                  min={1024}
                  max={65535}
                  required
                  value={form[key]}
                  onChange={(e) => setForm({ ...form, [key]: e.target.value })}
                  aria-label={`${label} port`}
                  className="font-mono"
                />
                <p className="text-xs text-muted-foreground">{help}</p>
              </div>
            ))}
          </div>
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-0.5">
              <Label htmlFor="auto-update">Install runtime updates automatically</Label>
              <p className="text-muted-foreground">
                New patch releases of PHP, databases, Node.js, Bun and Composer that RED installed.
              </p>
            </div>
            <Switch
              id="auto-update"
              checked={form.auto}
              onCheckedChange={(auto) => setForm({ ...form, auto })}
            />
          </div>
          <ErrorNote error={save.error} />
        </CardContent>
        <CardFooter className="mt-6 justify-end">
          <Button type="submit" disabled={save.isPending}>
            Save
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
