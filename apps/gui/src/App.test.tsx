import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { App, formatUptime } from "./App";
import { api, RedApiError } from "./api/client";
import type { StatusResponse } from "./api/gen/StatusResponse";

vi.mock("./api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api/client")>();
  return { ...actual, api: vi.fn() };
});

const status: StatusResponse = {
  version: "0.0.1",
  api_version: 1,
  os: "macos",
  pid: 4242,
  uptime_secs: 125,
  tld: "test",
  http_port: 8180,
  https_port: 8443,
  dns_port: 53530,
  paths: {
    config_dir: "/Users/me/Library/Application Support/RED",
    data_dir: "/Users/me/Library/Application Support/RED/data",
    socket: "/Users/me/Library/Application Support/RED/run/redd.sock",
    ca_cert: "/Users/me/Library/Application Support/RED/data/certs/ca.pem",
  },
};

function renderApp() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
}

describe("App", () => {
  it("shows daemon details when redd answers", async () => {
    vi.mocked(api).mockResolvedValue(status);
    renderApp();
    expect(await screen.findByText("redd is running")).toBeInTheDocument();
    expect(screen.getByText("pid 4242")).toBeInTheDocument();
    expect(screen.getByText(".test")).toBeInTheDocument();
    expect(screen.getByText("http://*.test:8180")).toBeInTheDocument();
    expect(screen.getByText("https://*.test:8443")).toBeInTheDocument();
    expect(screen.getByText("2m 5s")).toBeInTheDocument();
  });

  it("shows the hint when redd is not running", async () => {
    vi.mocked(api).mockRejectedValue(
      new RedApiError({
        code: "daemon_not_running",
        message: "redd is not running",
        hint: "Start it with `redd`.",
      }),
    );
    renderApp();
    expect(await screen.findByRole("alert")).toHaveTextContent("redd is not reachable");
    expect(screen.getByText("Start it with `redd`.")).toBeInTheDocument();
  });
});

describe("formatUptime", () => {
  it("formats seconds, minutes and hours", () => {
    expect(formatUptime(5)).toBe("5s");
    expect(formatUptime(125)).toBe("2m 5s");
    expect(formatUptime(7260)).toBe("2h 1m");
  });
});
