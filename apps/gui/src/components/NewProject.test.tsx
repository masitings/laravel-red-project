import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { ProjectJob } from "../api/gen/ProjectJob";
import { NewProject, projectName } from "./NewProject";

vi.mock("../api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/client")>();
  return { ...actual, api: vi.fn() };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const mysql = {
  name: "mysql",
  kind: "mysql",
  flavor: "mysql",
  version: "8.4",
  binary: "/b/mysqld",
  port: 3306,
  autostart: true,
  status: "running",
  pid: 1,
  error: null,
  data_dir: "/d",
  log_file: "/l",
  env: [],
};

let jobs: ProjectJob[];

function job(state: ProjectJob["state"]): ProjectJob {
  return {
    id: 1,
    name: "shop",
    kind: "wordpress",
    path: "/Users/me/Sites/shop",
    state,
    step: state === "done" ? "Ready" : "Downloading WordPress",
    log: ["==> Downloading WordPress"],
    url: state === "done" ? "https://shop.test" : null,
    error: null,
  };
}

function setup(services = [mysql]) {
  vi.mocked(api).mockImplementation(async (method: string, path: string) => {
    if (method === "POST" && path === "/v1/projects") {
      jobs = [job("running")];
      return jobs[0];
    }
    switch (path) {
      case "/v1/paths":
        return ["/Users/me/Sites"];
      case "/v1/services":
        return { services, available: [] };
      case "/v1/status":
        return { tld: "test" };
      case "/v1/projects":
        return jobs;
    }
    return null;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <NewProject onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  return client;
}

describe("NewProject", () => {
  beforeEach(() => {
    vi.mocked(api).mockReset();
    jobs = [];
  });

  it("turns any text into a project name", () => {
    expect(projectName("My Shop_2!")).toBe("my-shop-2-");
    expect(projectName("--a")).toBe("a");
  });

  it("creates WordPress on the MySQL service and follows it", async () => {
    const client = setup();
    fireEvent.click(await screen.findByText("WordPress"));
    fireEvent.change(screen.getByLabelText("Project name"), { target: { value: "Shop" } });
    expect(await screen.findByText("/Users/me/Sites")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Database")).toHaveValue("mysql"));
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("POST", "/v1/projects", {
        name: "shop",
        kind: "wordpress",
        directory: "/Users/me/Sites",
        secure: true,
        database: "mysql",
      }),
    );
    expect(await screen.findByText("Creating shop")).toBeInTheDocument();
    expect(screen.getByRole("log", { name: "Output" })).toHaveTextContent("Downloading WordPress");

    jobs = [job("done")];
    await client.invalidateQueries({ queryKey: ["projects"] });
    expect(await screen.findByRole("button", { name: "Open shop.test" })).toBeInTheDocument();
  });

  it("needs MySQL for WordPress", async () => {
    setup([]);
    fireEvent.click(await screen.findByText("WordPress"));
    fireEvent.change(screen.getByLabelText("Project name"), { target: { value: "blog" } });
    expect(await screen.findByText(/WordPress needs MySQL/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });
});
