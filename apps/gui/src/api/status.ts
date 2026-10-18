import { useQuery } from "@tanstack/react-query";
import { api } from "./client";
import type { StatusResponse } from "./gen/StatusResponse";

export const statusQueryKey = ["status"] as const;

/** Polls `GET /v1/status`. Becomes an SSE subscription in M1. */
export function useStatus() {
  return useQuery({
    queryKey: statusQueryKey,
    queryFn: () => api<StatusResponse>("GET", "/v1/status"),
    refetchInterval: 3000,
  });
}
