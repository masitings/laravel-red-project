import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import type { BackupInfo } from "./gen/BackupInfo";
import type { DatabasesResponse } from "./gen/DatabasesResponse";
import type { DoctorReport } from "./gen/DoctorReport";
import type { DumpsResponse } from "./gen/DumpsResponse";
import type { LogFile } from "./gen/LogFile";
import type { LogResponse } from "./gen/LogResponse";
import type { MailMessage } from "./gen/MailMessage";
import type { MailSummary } from "./gen/MailSummary";
import type { PhpListResponse } from "./gen/PhpListResponse";
import type { ProjectJob } from "./gen/ProjectJob";
import type { ProjectRequest } from "./gen/ProjectRequest";
import type { ProxyInfo } from "./gen/ProxyInfo";
import type { QueriesResponse } from "./gen/QueriesResponse";
import type { RuntimeKind } from "./gen/RuntimeKind";
import type { RuntimesResponse } from "./gen/RuntimesResponse";
import type { ServiceKind } from "./gen/ServiceKind";
import type { ServicesResponse } from "./gen/ServicesResponse";
import type { ServiceUpdate } from "./gen/ServiceUpdate";
import type { ShareInfo } from "./gen/ShareInfo";
import type { ShareLoginResponse } from "./gen/ShareLoginResponse";
import type { SharesResponse } from "./gen/SharesResponse";
import type { SiteInfo } from "./gen/SiteInfo";
import type { StatusResponse } from "./gen/StatusResponse";
import type { XdebugMode } from "./gen/XdebugMode";

export const keys = {
  status: ["status"] as const,
  sites: ["sites"] as const,
  proxies: ["proxies"] as const,
  php: ["php"] as const,
  doctor: ["doctor"] as const,
  services: ["services"] as const,
  mail: ["mail"] as const,
  dumps: ["dumps"] as const,
  logs: ["logs"] as const,
  runtimes: ["runtimes"] as const,
  projects: ["projects"] as const,
  queries: ["queries"] as const,
  shares: ["shares"] as const,
  databases: ["databases"] as const,
  paths: ["paths"] as const,
};

const enc = encodeURIComponent;

export function useStatus() {
  return useQuery({
    queryKey: keys.status,
    queryFn: () => api<StatusResponse>("GET", "/v1/status"),
    // Events keep data fresh; this only notices a daemon that went away.
    refetchInterval: 10_000,
  });
}

export function useSites() {
  return useQuery({ queryKey: keys.sites, queryFn: () => api<SiteInfo[]>("GET", "/v1/sites") });
}

export function useProxies() {
  return useQuery({
    queryKey: keys.proxies,
    queryFn: () => api<ProxyInfo[]>("GET", "/v1/proxies"),
  });
}

export function usePhp() {
  return useQuery({ queryKey: keys.php, queryFn: () => api<PhpListResponse>("GET", "/v1/php") });
}

export function useDoctor() {
  return useQuery({
    queryKey: keys.doctor,
    queryFn: () => api<DoctorReport>("GET", "/v1/doctor"),
  });
}

/** A mutation that refetches the given queries once it succeeds. */
function useAction<TArgs, TResult = unknown>(
  run: (args: TArgs) => Promise<TResult>,
  refresh: readonly (readonly string[])[],
) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: run,
    onSuccess: () => {
      for (const key of refresh) client.invalidateQueries({ queryKey: key });
    },
  });
}

export function useSiteActions() {
  const refresh = [keys.sites, keys.doctor];
  return {
    park: useAction((path: string) => api("POST", "/v1/paths", { path }), [...refresh, keys.paths]),
    link: useAction((path: string) => api("POST", "/v1/links", { path }), refresh),
    unlink: useAction((name: string) => api("DELETE", `/v1/links/${enc(name)}`), refresh),
    isolate: useAction(
      ({ name, version }: { name: string; version: string | null }) =>
        version === null
          ? api("DELETE", `/v1/sites/${enc(name)}/php`)
          : api("PUT", `/v1/sites/${enc(name)}/php`, { version }),
      refresh,
    ),
    secure: useAction(
      ({ name, on }: { name: string; on: boolean }) =>
        api(on ? "PUT" : "DELETE", `/v1/sites/${enc(name)}/secure`),
      refresh,
    ),
  };
}

export function useProxyActions() {
  const refresh = [keys.proxies];
  return {
    add: useAction(
      ({ name, target }: { name: string; target: string }) =>
        api("POST", "/v1/proxies", { name, target }),
      refresh,
    ),
    remove: useAction((name: string) => api("DELETE", `/v1/proxies/${enc(name)}`), refresh),
    secure: useAction(
      ({ name, on }: { name: string; on: boolean }) =>
        api(on ? "PUT" : "DELETE", `/v1/proxies/${enc(name)}/secure`),
      refresh,
    ),
  };
}

export function useSetDefaultPhp() {
  return useAction(
    (version: string) => api("PUT", "/v1/php/default", { version }),
    [keys.php, keys.sites],
  );
}

export function useSetXdebug() {
  return useAction(
    (mode: XdebugMode) => api("PUT", "/v1/xdebug", { mode }),
    [keys.php, keys.doctor],
  );
}

export function useServices() {
  return useQuery({
    queryKey: keys.services,
    queryFn: () => api<ServicesResponse>("GET", "/v1/services"),
    // Services move through "starting" on their own; poll while any does.
    refetchInterval: (query) =>
      query.state.data?.services.some((s) => s.status === "starting") ? 1000 : false,
  });
}

export function useServiceActions() {
  const refresh = [keys.services, keys.doctor];
  return {
    add: useAction(
      ({ kind, version, port }: { kind: ServiceKind; version?: string; port?: number }) =>
        api("POST", "/v1/services", { kind, version, port }),
      refresh,
    ),
    action: useAction(
      ({ name, action }: { name: string; action: "start" | "stop" | "restart" }) =>
        api("POST", `/v1/services/${enc(name)}/${action}`),
      refresh,
    ),
    autostart: useAction(
      ({ name, on }: { name: string; on: boolean }) =>
        api("PUT", `/v1/services/${enc(name)}`, { autostart: on }),
      refresh,
    ),
    update: useAction(
      ({ name, ...update }: ServiceUpdate & { name: string }) =>
        api("PUT", `/v1/services/${enc(name)}`, update),
      refresh,
    ),
    remove: useAction(
      ({ name, deleteData }: { name: string; deleteData: boolean }) =>
        api("DELETE", `/v1/services/${enc(name)}${deleteData ? "?delete_data=true" : ""}`),
      refresh,
    ),
  };
}

export function useMail() {
  return useQuery({ queryKey: keys.mail, queryFn: () => api<MailSummary[]>("GET", "/v1/mail") });
}

export function useMailMessage(id: string | null) {
  return useQuery({
    queryKey: [...keys.mail, id],
    queryFn: () => api<MailMessage>("GET", `/v1/mail/${enc(id ?? "")}`),
    enabled: id !== null,
  });
}

export function useMailActions() {
  const refresh = [keys.mail];
  return {
    remove: useAction((id: string) => api("DELETE", `/v1/mail/${enc(id)}`), refresh),
    clear: useAction(() => api("DELETE", "/v1/mail"), refresh),
  };
}

export function useDumps() {
  return useQuery({ queryKey: keys.dumps, queryFn: () => api<DumpsResponse>("GET", "/v1/dumps") });
}

export function useDumpActions() {
  const refresh = [keys.dumps];
  return {
    setEnabled: useAction(
      (enabled: boolean) => api("PUT", "/v1/dumps/settings", { enabled }),
      refresh,
    ),
    clear: useAction(() => api("DELETE", "/v1/dumps"), refresh),
  };
}

export function useLogs() {
  return useQuery({ queryKey: keys.logs, queryFn: () => api<LogFile[]>("GET", "/v1/logs") });
}

export function useLog(id: string | null, follow: boolean) {
  return useQuery({
    queryKey: [...keys.logs, id],
    queryFn: () => api<LogResponse>("GET", `/v1/logs/read?id=${enc(id ?? "")}&lines=500`),
    enabled: id !== null,
    refetchInterval: follow ? 2000 : false,
  });
}

export function useRuntimes() {
  return useQuery({
    queryKey: keys.runtimes,
    queryFn: () => api<RuntimesResponse>("GET", "/v1/runtimes"),
    // Events report progress; polling is a fallback while a download runs.
    refetchInterval: (query) =>
      query.state.data?.jobs.some((j) => j.state === "downloading" || j.state === "extracting")
        ? 1000
        : false,
  });
}

export function useRuntimeActions() {
  const refresh = [keys.runtimes, keys.php, keys.services, keys.doctor];
  return {
    install: useAction(
      ({ kind, version }: { kind: RuntimeKind; version: string }) =>
        api("POST", "/v1/runtimes", { kind, version }),
      refresh,
    ),
    remove: useAction(
      ({ kind, line }: { kind: RuntimeKind; line: string }) =>
        api("DELETE", `/v1/runtimes/${kind}/${enc(line)}`),
      refresh,
    ),
  };
}

export function useParkedPaths() {
  return useQuery({ queryKey: keys.paths, queryFn: () => api<string[]>("GET", "/v1/paths") });
}

export function useProjects() {
  return useQuery({
    queryKey: keys.projects,
    queryFn: () => api<ProjectJob[]>("GET", "/v1/projects"),
    // Events report progress; polling is a fallback while one runs.
    refetchInterval: (query) =>
      query.state.data?.some((j) => j.state === "running") ? 1500 : false,
  });
}

export function useCreateProject() {
  return useAction(
    (request: ProjectRequest) => api<ProjectJob>("POST", "/v1/projects", request),
    [keys.projects],
  );
}

export function useQueryLog() {
  return useQuery({
    queryKey: keys.queries,
    queryFn: () => api<QueriesResponse>("GET", "/v1/queries"),
  });
}

export function useQueryLogActions() {
  const refresh = [keys.queries];
  return {
    setEnabled: useAction(
      (enabled: boolean) => api("PUT", "/v1/queries/settings", { enabled }),
      refresh,
    ),
    clear: useAction(() => api("DELETE", "/v1/queries"), refresh),
  };
}

export function useShares() {
  return useQuery({
    queryKey: keys.shares,
    queryFn: () => api<SharesResponse>("GET", "/v1/shares"),
    // Events report progress; polling is a fallback while one starts.
    refetchInterval: (query) =>
      query.state.data?.shares.some((s) => s.state === "starting") ? 1500 : false,
  });
}

export function useShareActions() {
  const refresh = [keys.shares];
  return {
    start: useAction(
      ({ site, hostname }: { site: string; hostname?: string }) =>
        api<ShareInfo>("POST", `/v1/shares/${enc(site)}`, hostname ? { hostname } : {}),
      refresh,
    ),
    stop: useAction((site: string) => api("DELETE", `/v1/shares/${enc(site)}`), refresh),
    login: useAction(() => api<ShareLoginResponse>("POST", "/v1/shares/login"), refresh),
  };
}

export function useDatabases(service: string | null) {
  return useQuery({
    queryKey: [...keys.databases, service],
    queryFn: () => api<DatabasesResponse>("GET", `/v1/services/${enc(service ?? "")}/databases`),
    enabled: service !== null,
  });
}

export function useDatabaseActions(service: string) {
  const refresh = [keys.databases];
  const base = `/v1/services/${enc(service)}/databases`;
  return {
    create: useAction((name: string) => api("POST", base, { name }), refresh),
    drop: useAction((name: string) => api("DELETE", `${base}/${enc(name)}`), refresh),
    backup: useAction(
      (name: string) => api<BackupInfo>("POST", `${base}/${enc(name)}/backup`),
      refresh,
    ),
    restore: useAction(
      ({ name, file }: { name: string; file: string }) =>
        api("POST", `${base}/${enc(name)}/restore`, { file, create: true }),
      refresh,
    ),
  };
}
