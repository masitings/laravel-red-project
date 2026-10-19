import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { useState } from "react";
import { api } from "../api/client";
import type { RuntimeInfo } from "../api/gen/RuntimeInfo";
import type { RuntimeKind } from "../api/gen/RuntimeKind";
import type { StatusResponse } from "../api/gen/StatusResponse";
import { keys, useRuntimes } from "../api/queries";
import { Badge, Button, Card, ErrorNote } from "./ui";

const SIX_HOURS = 6 * 60 * 60 * 1000;

const label: Record<RuntimeKind, string> = {
  php: "PHP",
  mysql: "MySQL",
  postgres: "PostgreSQL",
  valkey: "Valkey",
  node: "Node.js",
  composer: "Composer",
};

/** Version of the desktop app, or `undefined` outside Tauri. */
export function useAppVersion() {
  return useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion(),
    enabled: isTauri(),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** Whether this build can update itself (released with a signing key). */
function useUpdaterEnabled() {
  return useQuery({
    queryKey: ["updater-enabled"],
    queryFn: () => invoke<boolean>("updater_enabled"),
    enabled: isTauri(),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** A newer release of the app, checked every six hours. `null` when none. */
export function useAppUpdate() {
  const enabled = useUpdaterEnabled();
  return useQuery({
    queryKey: ["app-update"],
    queryFn: () => check(),
    enabled: enabled.data === true,
    staleTime: SIX_HOURS,
    refetchInterval: SIX_HOURS,
    // `check()` returns a class instance; keep it as is.
    structuralSharing: false,
    retry: false,
  });
}

/** Runtimes RED installed that have an update. */
export function useRuntimeUpdates(): RuntimeInfo[] {
  const runtimes = useRuntimes();
  return runtimes.data?.installed.filter((i) => i.update !== null) ?? [];
}

/** How many things can be updated; shown next to the Status tab. */
export function useUpdateCount(): number {
  const app = useAppUpdate();
  return useRuntimeUpdates().length + (app.data ? 1 : 0);
}

export function UpdatesCard({ status }: { status: StatusResponse }) {
  return (
    <Card title="Updates">
      <div className="space-y-4 text-sm">
        {isTauri() && <AppUpdate status={status} />}
        <RuntimeUpdates />
      </div>
    </Card>
  );
}

function AppUpdate({ status }: { status: StatusResponse }) {
  const version = useAppVersion();
  const enabled = useUpdaterEnabled();
  const update = useAppUpdate();
  const client = useQueryClient();
  const [progress, setProgress] = useState<string | null>(null);
  const install = useMutation({
    mutationFn: async () => {
      const found = update.data;
      if (!found) return;
      let total = 0;
      let received = 0;
      await found.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          setProgress(
            total > 0
              ? `${Math.round((received / total) * 100)}%`
              : `${(received / 1_048_576).toFixed(1)} MB`,
          );
        }
        if (event.event === "Finished") setProgress("Restarting…");
      });
      await relaunch();
    },
  });
  const restart = useMutation({
    mutationFn: () => invoke<string>("restart_daemon"),
    onSuccess: async () => {
      await new Promise((resolve) => setTimeout(resolve, 1500));
      await client.invalidateQueries({ queryKey: keys.status });
    },
  });

  const app = version.data;
  const mismatch = app !== undefined && app !== status.version;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="font-medium">RED {app}</span>
          {enabled.data === false && (
            <span className="text-stone-500">
              This build does not update itself; download new releases from GitHub.
            </span>
          )}
          {enabled.data && update.isFetching && <span className="text-stone-500">Checking…</span>}
          {enabled.data && !update.isFetching && update.data === null && (
            <Badge tone="good">up to date</Badge>
          )}
          {update.data && <Badge tone="warn">{update.data.version} available</Badge>}
        </div>
        <div className="flex items-center gap-2">
          {enabled.data && !update.data && (
            <Button variant="ghost" onClick={() => update.refetch()} disabled={update.isFetching}>
              Check now
            </Button>
          )}
          {update.data && (
            <Button variant="primary" onClick={() => install.mutate()} disabled={install.isPending}>
              {install.isPending ? (progress ?? "Downloading…") : "Install and restart"}
            </Button>
          )}
        </div>
      </div>
      {update.data?.body && (
        <p className="whitespace-pre-line text-xs text-stone-500">{update.data.body}</p>
      )}
      <ErrorNote error={update.error ?? install.error} />
      {mismatch && (
        <div className="flex items-center justify-between gap-3 rounded-md bg-amber-50 p-2 dark:bg-amber-950">
          <span>
            redd {status.version} is running, but this app is {app}. Restart it to use the new
            version.
          </span>
          <Button onClick={() => restart.mutate()} disabled={restart.isPending}>
            {restart.isPending ? "Restarting…" : "Restart redd"}
          </Button>
        </div>
      )}
      <ErrorNote error={restart.error} />
    </div>
  );
}

function RuntimeUpdates() {
  const updates = useRuntimeUpdates();
  const client = useQueryClient();
  const updateAll = useMutation({
    mutationFn: async () => {
      for (const runtime of updates) {
        await api("POST", "/v1/runtimes", { kind: runtime.kind, version: runtime.line });
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: keys.runtimes }),
  });
  if (updates.length === 0) {
    return <p className="text-stone-500">Runtimes installed by RED are up to date.</p>;
  }
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <span className="font-medium">Runtime updates</span>
        <Button variant="primary" onClick={() => updateAll.mutate()} disabled={updateAll.isPending}>
          {updateAll.isPending ? "Starting…" : "Update all"}
        </Button>
      </div>
      <ul className="space-y-1">
        {updates.map((runtime) => (
          <li key={`${runtime.kind}-${runtime.line}`} className="flex gap-2">
            <span className="w-28">
              {label[runtime.kind]} {runtime.line}
            </span>
            <span className="font-mono text-xs leading-5 text-stone-600 dark:text-stone-400">
              {runtime.version} → {runtime.update}
              {runtime.update === runtime.version && " (RED build)"}
            </span>
          </li>
        ))}
      </ul>
      <ErrorNote error={updateAll.error} />
    </div>
  );
}
