import { invoke, isTauri } from "@tauri-apps/api/core";
import type { ApiError } from "./gen/ApiError";
import type { ErrorCode } from "./gen/ErrorCode";

/** Extra error codes produced on the GUI side, before a request reaches redd. */
export type ClientErrorCode = ErrorCode | "daemon_not_running" | "not_in_tauri" | "cancelled";

/** Error thrown by {@link api}. Mirrors the daemon's `ApiError` body. */
export class RedApiError extends Error {
  readonly code: ClientErrorCode;
  readonly hint?: string;
  readonly status?: number;

  constructor(body: { code: ClientErrorCode; message: string; hint?: string; status?: number }) {
    super(body.message);
    this.name = "RedApiError";
    this.code = body.code;
    this.hint = body.hint;
    this.status = body.status;
  }
}

type Method = "GET" | "POST" | "PATCH" | "PUT" | "DELETE";

/**
 * Call the Control API. The Tauri shell forwards the request to redd's unix
 * socket; the GUI never talks to the socket directly.
 */
export async function api<T>(method: Method, path: string, body?: unknown): Promise<T> {
  if (!isTauri()) {
    throw new RedApiError({
      code: "not_in_tauri",
      message: "The RED GUI talks to redd through the Tauri shell.",
      hint: "Run it with `pnpm tauri dev`.",
    });
  }
  try {
    return await invoke<T>("api", { method, path, body: body ?? null });
  } catch (error) {
    throw toRedApiError(error);
  }
}

function toRedApiError(error: unknown): RedApiError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    const body = error as ApiError & { code: ClientErrorCode; status?: number };
    return new RedApiError(body);
  }
  return new RedApiError({ code: "internal", message: String(error) });
}
