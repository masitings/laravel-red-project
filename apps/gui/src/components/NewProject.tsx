import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircle2Icon, ExternalLinkIcon, Loader2Icon, XCircleIcon } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import type { ProjectJob } from "@/api/gen/ProjectJob";
import type { ProjectKind } from "@/api/gen/ProjectKind";
import {
  useCreateProject,
  useParkedPaths,
  useProjects,
  useServices,
  useStatus,
} from "@/api/queries";
import { ErrorNote } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

const kinds: { kind: ProjectKind; label: string; help: string }[] = [
  { kind: "laravel", label: "Laravel", help: "laravel/laravel, SQLite unless you pick a database" },
  { kind: "laravel_react", label: "Laravel + React", help: "Starter kit with Inertia and React" },
  { kind: "laravel_vue", label: "Laravel + Vue", help: "Starter kit with Inertia and Vue" },
  { kind: "laravel_livewire", label: "Laravel + Livewire", help: "Starter kit with Livewire" },
  { kind: "wordpress", label: "WordPress", help: "Latest WordPress on a MySQL service" },
  { kind: "blank", label: "Empty site", help: "Just public/index.php" },
];

const SQLITE = "sqlite";

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
  const [database, setDatabase] = useState(SQLITE);
  const [secure, setSecure] = useState(true);
  const [jobId, setJobId] = useState<number | null>(initialJob);

  const folder = directory ?? paths.data?.[0] ?? null;
  const databases = (services.data?.services ?? []).filter(
    (s) => s.kind === "mysql" || (s.kind === "postgres" && kind !== "wordpress"),
  );
  // WordPress needs MySQL; default to the first one.
  const chosenDb =
    kind === "wordpress" && database === SQLITE ? (databases[0]?.name ?? "") : database;
  const job = projects.data?.find((j) => j.id === jobId);

  const choose = async () => {
    const picked = await pickFolder({ directory: true, multiple: false });
    if (typeof picked === "string") setDirectory(picked);
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!folder || !name) return;
    const created = await create.mutateAsync({
      name,
      kind,
      directory: folder,
      secure,
      database: chosenDb && chosenDb !== SQLITE ? chosenDb : undefined,
    });
    setJobId((created as ProjectJob).id);
  };

  if (jobId !== null) {
    return <Progress job={job} onClose={onClose} onAnother={() => setJobId(null)} />;
  }

  const wordpressWithoutMysql = kind === "wordpress" && databases.length === 0;
  return (
    <Card>
      <CardHeader>
        <CardTitle>New site</CardTitle>
        <CardDescription>
          RED creates the project, its database and mail settings, and serves it right away.
        </CardDescription>
      </CardHeader>
      <form onSubmit={submit}>
        <CardContent className="space-y-6">
          <fieldset className="space-y-2">
            <legend className="mb-2 text-sm font-medium">What to create</legend>
            <div className="grid grid-cols-2 gap-2 lg:grid-cols-3">
              {kinds.map((k) => (
                <label
                  key={k.kind}
                  className={cn(
                    "cursor-pointer rounded-lg border p-3 text-sm transition-colors has-[:focus-visible]:ring-[3px] has-[:focus-visible]:ring-ring/50",
                    kind === k.kind
                      ? "border-primary bg-primary/5"
                      : "hover:bg-accent hover:text-accent-foreground",
                  )}
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
                  <span className="block text-xs text-muted-foreground">{k.help}</span>
                </label>
              ))}
            </div>
          </fieldset>

          <div className="grid gap-6 md:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="project-name">Name</Label>
              <Input
                id="project-name"
                value={name}
                onChange={(e) => setName(projectName(e.target.value))}
                placeholder="my-app"
                required
                aria-label="Project name"
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                Folder and address: {name || "my-app"}.{tld}
              </p>
            </div>

            <div className="space-y-2">
              <Label>Folder</Label>
              <div className="flex items-center gap-2">
                <code className="flex h-9 min-w-0 flex-1 items-center truncate rounded-md border bg-muted/50 px-3 text-xs">
                  {folder ?? "Choose a folder"}
                </code>
                <Button type="button" variant="outline" onClick={choose}>
                  Choose…
                </Button>
              </div>
              {folder && !paths.data?.includes(folder) && (
                <p className="text-xs text-muted-foreground">
                  Not a parked folder; RED links the new project by itself.
                </p>
              )}
            </div>

            {kind !== "blank" && (
              <div className="space-y-2">
                <Label>Database</Label>
                <Select value={chosenDb} onValueChange={setDatabase}>
                  <SelectTrigger className="w-full" aria-label="Database">
                    <SelectValue placeholder="No MySQL service" />
                  </SelectTrigger>
                  <SelectContent>
                    {kind !== "wordpress" && (
                      <SelectItem value={SQLITE}>SQLite (a file in the project)</SelectItem>
                    )}
                    {databases.map((s) => (
                      <SelectItem key={s.name} value={s.name}>
                        {s.name} ({s.flavor ?? s.kind} on port {s.port})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {wordpressWithoutMysql && (
                  <p className="text-xs text-warning">
                    WordPress needs MySQL. Add a MySQL service on the Services page first.
                  </p>
                )}
              </div>
            )}

            <div className="flex items-center gap-3 self-end pb-2">
              <Switch id="project-https" checked={secure} onCheckedChange={setSecure} />
              <Label htmlFor="project-https">Serve over HTTPS</Label>
            </div>
          </div>
          <ErrorNote error={create.error} />
        </CardContent>
        <CardFooter className="mt-6 justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            disabled={!name || !folder || create.isPending || wordpressWithoutMysql}
          >
            Create
          </Button>
        </CardFooter>
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
  if (!job) {
    return (
      <Card>
        <CardContent className="text-sm text-muted-foreground">Starting…</CardContent>
      </Card>
    );
  }
  return (
    <Card>
      <CardHeader>
        <CardTitle>Creating {job.name}</CardTitle>
        <CardDescription className="truncate" title={job.path}>
          {job.path}
        </CardDescription>
        <CardAction>
          {job.state === "running" ? (
            <Badge variant="secondary">
              <Loader2Icon className="animate-spin" /> {job.step}
            </Badge>
          ) : job.state === "done" ? (
            <Badge variant="success">
              <CheckCircle2Icon /> Ready
            </Badge>
          ) : (
            <Badge variant="destructive">
              <XCircleIcon /> Failed
            </Badge>
          )}
        </CardAction>
      </CardHeader>
      <CardContent className="space-y-3">
        <pre
          ref={log}
          role="log"
          aria-label="Output"
          className="h-60 overflow-auto rounded-md bg-neutral-950 p-3 font-mono text-xs leading-relaxed text-neutral-200"
        >
          {job.log.join("\n") || " "}
        </pre>
        {job.state === "failed" && <ErrorNote error={new Error(job.error ?? "failed")} />}
      </CardContent>
      <CardFooter className="justify-end gap-2">
        {job.state !== "running" && (
          <Button variant="ghost" onClick={onAnother}>
            {job.state === "failed" ? "Try again" : "Create another"}
          </Button>
        )}
        <Button variant="outline" onClick={onClose}>
          {job.state === "running" ? "Hide" : "Close"}
        </Button>
        {job.url && (
          <Button onClick={() => job.url && openUrl(job.url)}>
            <ExternalLinkIcon /> Open {job.url.replace(/^https?:\/\//, "")}
          </Button>
        )}
      </CardFooter>
    </Card>
  );
}
