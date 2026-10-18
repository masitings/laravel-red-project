import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import type { DoctorReport } from "./gen/DoctorReport";
import type { PhpListResponse } from "./gen/PhpListResponse";
import type { ProxyInfo } from "./gen/ProxyInfo";
import type { SiteInfo } from "./gen/SiteInfo";
import type { StatusResponse } from "./gen/StatusResponse";

export const keys = {
  status: ["status"] as const,
  sites: ["sites"] as const,
  proxies: ["proxies"] as const,
  php: ["php"] as const,
  doctor: ["doctor"] as const,
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
function useAction<TArgs>(
  run: (args: TArgs) => Promise<unknown>,
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
    park: useAction((path: string) => api("POST", "/v1/paths", { path }), refresh),
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
