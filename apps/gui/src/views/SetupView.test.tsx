import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { Check } from "../api/gen/Check";
import type { StatusResponse } from "../api/gen/StatusResponse";
import { needsSetup, SetupView } from "./SetupView";

vi.mock("../api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/client")>();
  return { ...actual, api: vi.fn() };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => true }));

const status = {
  version: "0.4.0",
  api_version: 1,
  os: "macos",
  pid: 1,
  uptime_secs: 1,
  tld: "test",
  http_port: 8180,
  https_port: 8443,
  dns_port: 53530,
  helper: false,
  smtp_port: 2525,
  dump_port: 9912,
  paths: { config_dir: "/c", data_dir: "/d", socket: "/s", ca_cert: "/d/ca.pem" },
} satisfies StatusResponse;

const check = (id: string, status: Check["status"], detail = ""): Check => ({
  id,
  label: id,
  status,
  detail,
});

let checks: Check[];

function setup() {
  vi.mocked(api).mockImplementation(async (method: string, path: string) => {
    if (path === "/v1/doctor") return { checks };
    if (path === "/v1/runtimes")
      return {
        platform: "macos-aarch64",
        available: [],
        installed: [],
        jobs: [],
        catalog_errors: [],
      };
    if (method === "POST" && path === "/v1/dns") {
      checks = checks.map((c) => (c.id === "resolver" ? check("resolver", "ok") : c));
      return null;
    }
    return null;
  });
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    if (command === "cli_status") return { bundled: "/Applications/RED.app/red", installed: null };
    if (command === "setup_step") {
      const step = (args as { step: string }).step;
      if (step === "system") throw { code: "cancelled", message: "cancelled" };
      return "";
    }
    return null;
  });
  const onClose = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SetupView status={status} onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

describe("SetupView", () => {
  beforeEach(() => {
    vi.mocked(api).mockReset();
    vi.mocked(invoke).mockReset();
    checks = [
      check("php", "ok", "8.4.12"),
      check("helper", "warn", "not installed"),
      check("autostart", "warn", "not set up"),
      check("resolver", "fail", "missing"),
      check("ca", "fail", "not trusted"),
    ];
  });

  it("knows when setup is needed", () => {
    expect(needsSetup(undefined)).toBe(false);
    expect(needsSetup([check("php", "ok"), check("dns", "fail")])).toBe(false);
    expect(needsSetup([check("ca", "warn")])).toBe(true);
  });

  it("shows done steps and runs the others", async () => {
    setup();
    expect(await screen.findByText(/1 of 6 done/)).toBeInTheDocument();
    const php = screen.getByRole("listitem", { name: "Install PHP" });
    expect(within(php).getByText("Done")).toBeInTheDocument();

    // Autostart waits for the helper.
    const autostart = screen.getByRole("listitem", { name: "Start RED at login" });
    expect(within(autostart).getByRole("button", { name: "Start at login" })).toBeDisabled();

    // Closing the password dialog shows no error.
    fireEvent.click(screen.getByRole("button", { name: "Install helper" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("setup_step", { step: "system" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Install helper" })).toBeEnabled(),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Trust certificates" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("setup_step", { step: "trust" }));
  });

  it("can be skipped", async () => {
    const onClose = setup();
    fireEvent.click(await screen.findByRole("button", { name: "Skip for now" }));
    expect(onClose).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Done" })).toBeDisabled();
  });

  it("sets up DNS once the helper runs", async () => {
    checks = checks.map((c) => (c.id === "helper" ? check("helper", "ok") : c));
    setup();
    const button = await screen.findByRole("button", { name: "Set up DNS" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() => expect(api).toHaveBeenCalledWith("POST", "/v1/dns"));
    const dns = await screen.findByRole("listitem", { name: "Use .test domains" });
    await waitFor(() => expect(within(dns).getByText("Done")).toBeInTheDocument());
  });
});
