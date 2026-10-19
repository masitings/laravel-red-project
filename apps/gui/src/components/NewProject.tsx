import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type FormEvent, useEffect, useRef, useState } from "react";
import type { ProjectJob } from "../api/gen/ProjectJob";
import type { ProjectKind } from "../api/gen/ProjectKind";
import {
  useCreateProject,
  useParkedPaths,
  useProjects,
  useServices,
  useStatus,
} from "../api/queries";
import { Badge, Button, Card, ErrorNote } from "./ui";

const kinds: { kind: ProjectKind; label: string; help: string }[] = [
  { kind: "laravel", label: "Laravel", help: "laravel/laravel, SQLite unless you pick a database" },
  { kind: "laravel_react", label: "Laravel + React", help: "Starter kit with Inertia and React" },
  { kind: "laravel_vue", label: "Laravel + Vue", help: "Starter kit with Inertia and Vue" },
  { kind: "laravel_livewire", label: "Laravel + Livewire", help: "Starter kit with Livewire" },
  { kind: "wordpress", label: "WordPress", help: "Latest WordPress on a MySQL service" },
  { kind: "blank", label: "Empty site", help: "Just public/index.php" },
];

/** Lowercase letters, digits and `-`, as the daemon accepts. */
export function projectName(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-/, "")
    .slice(0, 63);
}

/**
 * "New site": create a Laravel, WordPress or empty project in a folder,
 * with its database and mail set up, then follow the job until it is served.
 */
export function NewProject({
  onClose,
  jobId: initialJob = null,
}: {
  onClose: () => void;
  /** Show this creation's progress instead of the form. */
  jobId?: number | null;
}) {
  const tld = useStatus().data?.tld ?? "test";
  const paths = useParkedPaths();
  const services = useServices();
  const create = useCreateProject();
  const projects = useProjects();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<ProjectKind>("laravel");
  const [directory, setDirectory] = useState<string | null>(null);
  const [database, setDatabase] = useState("");
  const [secure, setSecure] = useState(true);
  const [jobId, setJobId] = useState<number | null>(initialJob);

  const folder = directory ?? paths.data?.[0] ?? null;
  const databases = (services.data?.services ?? []).filter(
    (s) => s.kind === "mysql" || (s.kind === "postgres" && kind !== "wordpress"),
  );
  // WordPress needs MySQL; default to the first one.
  const chosenDb = kind === "wordpress" && database === "" ? (databases[0]?.name ?? "") : database;
  const job = projects.data?.find((j) => j.id === jobId);

  const choose = async () => {
    const picked = await pickFolder({ directory: true, multiple: false });
    if (typeof picked === "string") setDirectory(picked);
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!folder || !name) return;
    const created = (await create.mutateAsync({
      name,
      kind,
      directory: folder,
      secure,
      database: chosenDb || undefined,
    })) as ProjectJob;
    setJobId(created.id);
  };

  if (jobId !== null) {
    return <Progress job={job} onClose={onClose} onAnother={() => setJobId(null)} />;
  }

  const wordpressWithoutMysql = kind === "wordpress" && databases.length === 0;
  return (
    <Card title="New site">
      <form onSubmit={submit} className="space-y-4 text-sm">
        <fieldset>
          <legend className="mb-2 font-medium">What to create</legend>
          <div className="grid grid-cols-2 gap-2 lg:grid-cols-3">
            {kinds.map((k) => (
              <label
                key={k.kind}
                className={`cursor-pointer rounded-lg border p-3 has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-red-500 ${
                  kind === k.kind
                    ? "border-red-600 bg-red-50 dark:bg-red-950/40"
                    : "border-stone-200 hover:bg-stone-50 dark:border-stone-700 dark:hover:bg-stone-800"
                }`}
              >
                <input
                  type="radio"
                  name="project-kind"
                  value={k.kind}
                  className="sr-only"
                  checked={kind === k.kind}
                  onChange={() => setKind(k.kind)}
                />
                <span className="block font-medium">{k.label}</span>
                <span className="block text-xs text-stone-500">{k.help}</span>
              </label>
            ))}
          </div>
        </fieldset>

        <label className="block space-y-1">
          <span className="font-medium">Name</span>
          <input
            value={name}
            onChange={(e) => setName(projectName(e.target.value))}
            placeholder="my-app"
            required
            aria-label="Project name"
            className="w-full rounded-md border border-stone-300 bg-white px-2 py-1.5 font-mono dark:border-stone-700 dark:bg-stone-950"
          />
          <span className="block text-xs text-stone-500">
            Folder and address: {name || "my-app"}.{tld}
          </span>
        </label>

        <div className="space-y-1">
          <span className="block font-medium">Folder</span>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-md bg-stone-100 px-2 py-1.5 dark:bg-stone-800">
              {folder ?? "Choose a folder"}
            </code>
            <Button onClick={choose}>Choose…</Button>
          </div>
          {folder && !paths.data?.includes(folder) && (
            <span className="block text-xs text-stone-500">
              Not a parked folder; RED links the new project by itself.
            </span>
          )}
        </div>

        {kind !== "blank" && (
          <label className="block space-y-1">
            <span className="font-medium">Database</span>
            <select
              value={chosenDb}
              onChange={(e) => setDatabase(e.target.value)}
              aria-label="Database"
              className="w-full rounded-md border border-stone-300 bg-white px-2 py-1.5 dark:border-stone-700 dark:bg-stone-950"
            >
              {kind !== "wordpress" && <option value="">SQLite (a file in the project)</option>}
              {databases.map((s) => (
                <option key={s.name} value={s.name}>
                  {s.name} ({s.flavor ?? s.kind} on port {s.port})
                </option>
              ))}
            </select>
            {wordpressWithoutMysql && (
              <span className="block text-xs text-amber-700 dark:text-amber-400">
                WordPress needs MySQL. Add a MySQL service on the Services page first.
              </span>
            )}
          </label>
        )}

        <label className="flex items-center gap-2">
          <input type="checkbox" checked={secure} onChange={(e) => setSecure(e.target.checked)} />
          Serve over HTTPS
        </label>

        <ErrorNote error={create.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant="primary"
            disabled={!name || !folder || create.isPending || wordpressWithoutMysql}
          >
            Create
          </Button>
        </div>
      </form>
    </Card>
  );
}

function Progress({
  job,
  onClose,
  onAnother,
}: {
  job: ProjectJob | undefined;
  onClose: () => void;
  onAnother: () => void;
}) {
  const log = useRef<HTMLPreElement>(null);
  const lines = job?.log.length ?? 0;
  // Keep the newest output in view.
  useEffect(() => {
    if (lines && log.current) log.current.scrollTop = log.current.scrollHeight;
  }, [lines]);
  if (!job) return <Card title="New site">Starting…</Card>;
  return (
    <Card
      title={`Creating ${job.name}`}
      actions={
        job.state === "running" ? (
          <Badge>{job.step}</Badge>
        ) : job.state === "done" ? (
          <Badge tone="good">Ready</Badge>
        ) : (
          <Badge tone="bad">Failed</Badge>
        )
      }
    >
      <div className="space-y-3 text-sm">
        <p className="truncate text-stone-500" title={job.path}>
          {job.path}
        </p>
        <pre
          ref={log}
          role="log"
          aria-label="Output"
          className="h-56 overflow-auto rounded-md bg-stone-950 p-3 font-mono text-xs leading-relaxed text-stone-200"
        >
          {job.log.join("\n") || " "}
        </pre>
        {job.state === "failed" && (
          <p role="alert" className="text-red-700 dark:text-red-400">
            {job.error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          {job.state !== "running" && (
            <Button variant="ghost" onClick={onAnother}>
              {job.state === "failed" ? "Try again" : "Create another"}
            </Button>
          )}
          <Button onClick={onClose}>{job.state === "running" ? "Hide" : "Close"}</Button>
          {job.url && (
            <Button variant="primary" onClick={() => job.url && openUrl(job.url)}>
              Open {job.url.replace(/^https?:\/\//, "")}
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}
