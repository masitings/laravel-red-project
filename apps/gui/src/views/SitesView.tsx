import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type FormEvent, useState } from "react";
import type { SiteInfo } from "../api/gen/SiteInfo";
import {
  usePhp,
  useProjects,
  useProxies,
  useProxyActions,
  useSiteActions,
  useSites,
} from "../api/queries";
import { NewProject } from "../components/NewProject";
import { Badge, Button, Card, ErrorNote } from "../components/ui";

async function chooseFolder(): Promise<string | null> {
  const picked = await pickFolder({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

export function SitesView() {
  // `undefined`: closed; `null`: the form; a number: that creation's progress.
  const [creating, setCreating] = useState<number | null | undefined>(undefined);
  const projects = useProjects();
  const running = projects.data?.filter((j) => j.state === "running") ?? [];
  return (
    <div className="space-y-6">
      {creating !== undefined && (
        <NewProject jobId={creating} onClose={() => setCreating(undefined)} />
      )}
      {creating === undefined &&
        running.map((job) => (
          <div
            key={job.id}
            className="flex items-center justify-between gap-3 rounded-xl border border-stone-200 bg-white px-4 py-3 text-sm dark:border-stone-800 dark:bg-stone-900"
          >
            <span>
              Creating <strong>{job.name}</strong>: {job.step}
            </span>
            <Button onClick={() => setCreating(job.id)}>Show</Button>
          </div>
        ))}
      <SitesCard onNew={creating === undefined ? () => setCreating(null) : undefined} />
      <ProxiesCard />
    </div>
  );
}

function SitesCard({ onNew }: { onNew?: () => void }) {
  const sites = useSites();
  const actions = useSiteActions();
  const pending = Object.values(actions).find((a) => a.isPending);
  const failed = Object.values(actions).find((a) => a.error);

  const park = async () => {
    const path = await chooseFolder();
    if (path) actions.park.mutate(path);
  };
  const link = async () => {
    const path = await chooseFolder();
    if (path) actions.link.mutate(path);
  };

  return (
    <Card
      title="Sites"
      actions={
        <>
          {onNew && (
            <Button variant="primary" onClick={onNew}>
              New site…
            </Button>
          )}
          <Button onClick={park} disabled={Boolean(pending)}>
            Park folder…
          </Button>
          <Button onClick={link} disabled={Boolean(pending)}>
            Link folder…
          </Button>
        </>
      }
    >
      <ErrorNote error={failed?.error ?? sites.error} />
      {sites.data?.length === 0 && (
        <p className="text-sm text-stone-500">
          No sites yet. Create one with New site, park a folder that contains your projects, or link
          a single project.
        </p>
      )}
      {sites.data && sites.data.length > 0 && (
        <table className="w-full text-left text-sm">
          <thead className="text-xs text-stone-500">
            <tr>
              <th className="py-2 font-medium">Site</th>
              <th className="font-medium">Driver</th>
              <th className="font-medium">PHP</th>
              <th className="font-medium">HTTPS</th>
              <th className="font-medium" />
            </tr>
          </thead>
          <tbody>
            {sites.data.map((site) => (
              <SiteRow key={site.name} site={site} />
            ))}
          </tbody>
        </table>
      )}
    </Card>
  );
}

function SiteRow({ site }: { site: SiteInfo }) {
  const actions = useSiteActions();
  const php = usePhp();
  const versions = php.data?.installs.map((i) => i.version) ?? [];
  return (
    <tr className="border-t border-stone-100 align-top dark:border-stone-800">
      <td className="py-2 pr-3">
        <button
          type="button"
          className="font-medium text-red-700 hover:underline dark:text-red-400"
          onClick={() => openUrl(site.url)}
        >
          {site.url.replace(/^https?:\/\//, "")}
        </button>
        <div className="max-w-xs truncate text-xs text-stone-500" title={site.path}>
          {site.path}
        </div>
      </td>
      <td className="py-2 pr-3">
        <Badge>{site.driver}</Badge>
      </td>
      <td className="py-2 pr-3">
        <select
          aria-label={`PHP version for ${site.name}`}
          className="rounded border border-stone-300 bg-transparent px-1 py-0.5 dark:border-stone-700"
          value={site.isolated ? (site.php ?? "") : ""}
          onChange={(e) =>
            actions.isolate.mutate({ name: site.name, version: e.target.value || null })
          }
        >
          <option value="">Default{!site.isolated && site.php ? ` (${site.php})` : ""}</option>
          {versions.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      </td>
      <td className="py-2 pr-3">
        <Button
          variant={site.secure ? "secondary" : "ghost"}
          aria-pressed={site.secure}
          onClick={() => actions.secure.mutate({ name: site.name, on: !site.secure })}
        >
          {site.secure ? "On" : "Off"}
        </Button>
      </td>
      <td className="py-2 text-right">
        {site.kind === "linked" ? (
          <Button variant="ghost" onClick={() => actions.unlink.mutate(site.name)}>
            Unlink
          </Button>
        ) : (
          <span className="text-xs text-stone-400">parked</span>
        )}
      </td>
    </tr>
  );
}

function ProxiesCard() {
  const proxies = useProxies();
  const actions = useProxyActions();
  const [name, setName] = useState("");
  const [target, setTarget] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    actions.add.mutate(
      { name, target },
      {
        onSuccess: () => {
          setName("");
          setTarget("");
        },
      },
    );
  };

  return (
    <Card title="Proxies">
      <p className="mb-3 text-sm text-stone-500">
        Forward a <code>.test</code> name to a local dev server, WebSockets included (Vite, Next.js,
        Reverb).
      </p>
      <ErrorNote error={actions.add.error ?? actions.remove.error ?? actions.secure.error} />
      <form onSubmit={submit} className="mb-3 flex flex-wrap gap-2">
        <input
          aria-label="Proxy name"
          placeholder="name, e.g. app"
          className="rounded border border-stone-300 bg-transparent px-2 py-1 text-sm dark:border-stone-700"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <input
          aria-label="Proxy target"
          placeholder="port or URL, e.g. 5173"
          className="rounded border border-stone-300 bg-transparent px-2 py-1 text-sm dark:border-stone-700"
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          required
        />
        <Button type="submit" variant="primary" disabled={actions.add.isPending}>
          Add proxy
        </Button>
      </form>
      <ul className="divide-y divide-stone-100 text-sm dark:divide-stone-800">
        {proxies.data?.map((proxy) => (
          <li key={proxy.name} className="flex items-center justify-between gap-3 py-2">
            <div>
              <button
                type="button"
                className="font-medium text-red-700 hover:underline dark:text-red-400"
                onClick={() => openUrl(proxy.url)}
              >
                {proxy.url.replace(/^https?:\/\//, "")}
              </button>
              <span className="text-stone-500"> → {proxy.target}</span>
            </div>
            <div className="flex gap-2">
              <Button
                variant="ghost"
                aria-pressed={proxy.secure}
                onClick={() => actions.secure.mutate({ name: proxy.name, on: !proxy.secure })}
              >
                {proxy.secure ? "HTTPS" : "HTTP"}
              </Button>
              <Button variant="ghost" onClick={() => actions.remove.mutate(proxy.name)}>
                Remove
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </Card>
  );
}
