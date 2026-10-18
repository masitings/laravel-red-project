import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App, formatUptime } from "./App";
import { api, RedApiError } from "./api/client";
import type { DoctorReport } from "./api/gen/DoctorReport";
import type { DumpsResponse } from "./api/gen/DumpsResponse";
import type { LogFile } from "./api/gen/LogFile";
import type { LogResponse } from "./api/gen/LogResponse";
import type { MailMessage } from "./api/gen/MailMessage";
import type { MailSummary } from "./api/gen/MailSummary";
import type { PhpListResponse } from "./api/gen/PhpListResponse";
import type { ServicesResponse } from "./api/gen/ServicesResponse";
import type { SiteInfo } from "./api/gen/SiteInfo";
import type { StatusResponse } from "./api/gen/StatusResponse";

vi.mock("./api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api/client")>();
  return { ...actual, api: vi.fn() };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const status: StatusResponse = {
  version: "0.1.0",
  api_version: 1,
  os: "macos",
  pid: 4242,
  uptime_secs: 125,
  tld: "test",
  http_port: 80,
  https_port: 8443,
  dns_port: 53530,
  helper: false,
  smtp_port: 2525,
  dump_port: 9912,
  paths: {
    config_dir: "/Users/me/Library/Application Support/RED",
    data_dir: "/Users/me/Library/Application Support/RED/data",
    socket: "/Users/me/Library/Application Support/RED/run/redd.sock",
    ca_cert: "/Users/me/Library/Application Support/RED/data/certs/ca.pem",
  },
};

const sites: SiteInfo[] = [
  {
    name: "blog",
    path: "/Users/me/Sites/blog",
    url: "https://blog.test",
    kind: "parked",
    driver: "laravel",
    php: "8.4",
    isolated: false,
    secure: true,
    cert_expires: 1_900_000_000,
  },
  {
    name: "shop",
    path: "/Users/me/code/shop",
    url: "http://shop.test",
    kind: "linked",
    driver: "wordpress",
    php: "8.2",
    isolated: true,
    secure: false,
    cert_expires: null,
  },
];

const php: PhpListResponse = {
  default: "8.4",
  installs: [
    {
      version: "8.4",
      full_version: "8.4.12",
      fpm_path: "/opt/homebrew/opt/php/sbin/php-fpm",
      running: true,
    },
    {
      version: "8.2",
      full_version: "8.2.29",
      fpm_path: "/opt/homebrew/opt/php@8.2/sbin/php-fpm",
      running: false,
    },
  ],
};

const doctor: DoctorReport = {
  checks: [
    { id: "php", label: "PHP", status: "ok", detail: "8.4.12, 8.2.29" },
    {
      id: "ports",
      label: "Web ports",
      status: "warn",
      detail: "serving on 80 and 8443",
      fix: "red helper:install",
    },
  ],
};

const services: ServicesResponse = {
  services: [
    {
      name: "mysql",
      kind: "mysql",
      flavor: "mariadb",
      version: "11.4",
      binary: "/opt/homebrew/opt/mariadb/bin/mariadbd",
      port: 3306,
      autostart: true,
      status: "failed",
      pid: null,
      error: "port 3306 is already in use",
      data_dir: "/data/services/mysql/data",
      log_file: "/logs/services/mysql.log",
      env: [{ name: "DB_PORT", value: "3306" }],
    },
  ],
  available: [
    {
      kind: "postgres",
      flavor: "postgresql",
      version: "17.2",
      full_version: "17.2",
      binary: "/opt/homebrew/opt/postgresql@17/bin/postgres",
    },
  ],
};

const summary: MailSummary = {
  id: "1760000000000-0001",
  mailbox: "blog",
  from: "Blog <app@blog.test>",
  to: ["user@example.test"],
  subject: "Welcome aboard",
  received: 1_760_000_000,
  size: 512,
  has_html: true,
  attachments: 1,
  read: false,
};

const message: MailMessage = {
  summary: { ...summary, read: true },
  cc: [],
  envelope_to: ["user@example.test"],
  text: "Hello there",
  html: "<p>Hello there</p>",
  headers: [{ name: "Subject", value: "Welcome aboard" }],
  attachments: [{ index: 0, filename: "invoice.pdf", content_type: "application/pdf", size: 2048 }],
};

const dumps: DumpsResponse = {
  enabled: false,
  port: 9912,
  dumps: [
    {
      id: 1,
      received: 1_760_000_000_000,
      html: "<pre>array:1</pre>",
      source: { file: "/app/routes/web.php", line: 7, name: "web.php" },
      origin: "GET /",
    },
  ],
};

const laravelLog: LogFile = {
  id: "site:blog:storage/logs/laravel.log",
  kind: "site",
  label: "blog · laravel.log",
  path: "/Users/me/Sites/blog/storage/logs/laravel.log",
  size: 100,
  modified: 1_760_000_000,
};
const logs: LogFile[] = [laravelLog];

const log: LogResponse = {
  file: laravelLog,
  entries: [
    { time: "2026-10-18 10:00:00", level: "error", channel: "local", message: "Boom" },
    { time: "2026-10-18 10:00:01", level: "info", channel: "local", message: "All good" },
  ],
  truncated: false,
};

function mockDaemon() {
  vi.mocked(api).mockImplementation(async (method: string, path: string) => {
    if (method !== "GET") return null;
    switch (path) {
      case "/v1/status":
        return status;
      case "/v1/sites":
        return sites;
      case "/v1/proxies":
        return [];
      case "/v1/php":
        return php;
      case "/v1/doctor":
        return doctor;
      case "/v1/services":
        return services;
      case "/v1/mail":
        return [summary];
      case `/v1/mail/${summary.id}`:
        return message;
      case "/v1/dumps":
        return dumps;
      case "/v1/logs":
        return logs;
      case `/v1/logs/read?id=${encodeURIComponent(laravelLog.id)}&lines=500`:
        return log;
      default:
        throw new Error(`unexpected ${path}`);
    }
  });
}

function renderApp() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
}

describe("App", () => {
  // A function returned from beforeEach is treated as cleanup, so use braces.
  beforeEach(() => {
    vi.mocked(api).mockReset();
  });

  it("lists sites with driver, PHP and HTTPS state", async () => {
    mockDaemon();
    renderApp();
    expect(await screen.findByText("blog.test")).toBeInTheDocument();
    expect(screen.getByText("laravel")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("PHP version for shop")).toHaveValue("8.2"));
    expect(screen.getByText("Unlink")).toBeInTheDocument();
  });

  it("isolates a site when a PHP version is picked", async () => {
    mockDaemon();
    renderApp();
    const select = await screen.findByLabelText("PHP version for blog");
    await waitFor(() =>
      expect(screen.getAllByRole("option", { name: "8.2" }).length).toBeGreaterThan(0),
    );
    fireEvent.change(select, { target: { value: "8.2" } });
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("PUT", "/v1/sites/blog/php", { version: "8.2" }),
    );
  });

  it("adds a proxy", async () => {
    mockDaemon();
    renderApp();
    fireEvent.change(await screen.findByLabelText("Proxy name"), { target: { value: "vite" } });
    fireEvent.change(screen.getByLabelText("Proxy target"), { target: { value: "5173" } });
    fireEvent.click(screen.getByText("Add proxy"));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("POST", "/v1/proxies", { name: "vite", target: "5173" }),
    );
  });

  it("shows PHP versions and the default", async () => {
    mockDaemon();
    renderApp();
    fireEvent.click(await screen.findByText("PHP"));
    expect(await screen.findByText("PHP 8.4.12")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Make default"));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("PUT", "/v1/php/default", { version: "8.2" }),
    );
  });

  it("shows status and doctor checks with fixes", async () => {
    mockDaemon();
    renderApp();
    fireEvent.click(await screen.findByText("Status"));
    expect(await screen.findByText("redd is running · pid 4242")).toBeInTheDocument();
    expect(screen.getByText("https://*.test:8443")).toBeInTheDocument();
    expect(await screen.findByText("red helper:install")).toBeInTheDocument();
  });

  it("shows failed services and adds a new one", async () => {
    mockDaemon();
    renderApp();
    fireEvent.click(await screen.findByText("Services"));
    expect(await screen.findByText(/port 3306 is already in use/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Server"), { target: { value: "0" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "5433" } });
    fireEvent.click(screen.getByText("Add service"));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("POST", "/v1/services", {
        kind: "postgres",
        version: "17.2",
        port: 5433,
      }),
    );
    fireEvent.click(screen.getByText("Start"));
    await waitFor(() => expect(api).toHaveBeenCalledWith("POST", "/v1/services/mysql/start"));
  });

  it("shows caught mail with an unread count", async () => {
    mockDaemon();
    renderApp();
    expect(await screen.findByText("unread")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Mail"));
    fireEvent.click(await screen.findByText("Welcome aboard"));
    expect(await screen.findByText("From Blog <app@blog.test>")).toBeInTheDocument();
    expect(screen.getByText("invoice.pdf · 2 KB")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "text" }));
    expect(screen.getByText("Hello there")).toBeInTheDocument();
  });

  it("turns dump capturing on", async () => {
    mockDaemon();
    renderApp();
    fireEvent.click(await screen.findByText("Dumps"));
    expect(await screen.findByText("web.php:7")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Capture dumps"));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("PUT", "/v1/dumps/settings", { enabled: true }),
    );
  });

  it("shows and filters log entries", async () => {
    mockDaemon();
    renderApp();
    fireEvent.click(await screen.findByText("Logs"));
    expect(await screen.findByText("Boom")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Level"), { target: { value: "info" } });
    expect(screen.queryByText("Boom")).not.toBeInTheDocument();
    expect(screen.getByText("All good")).toBeInTheDocument();
  });

  it("explains how to start redd when it is not running", async () => {
    vi.mocked(api).mockImplementation(async () => {
      throw new RedApiError({
        code: "daemon_not_running",
        message: "redd is not running",
        hint: "Start the daemon with `redd`.",
      });
    });
    renderApp();
    expect(await screen.findByRole("alert")).toHaveTextContent("redd is not reachable");
    expect(screen.getByText("Start the daemon with `redd`.")).toBeInTheDocument();
  });
});

describe("formatUptime", () => {
  it("formats seconds, minutes and hours", () => {
    expect(formatUptime(5)).toBe("5s");
    expect(formatUptime(125)).toBe("2m 5s");
    expect(formatUptime(7260)).toBe("2h 1m");
  });
});
