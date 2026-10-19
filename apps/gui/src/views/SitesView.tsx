import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  CopyIcon,
  ExternalLinkIcon,
  FolderOpenIcon,
  FolderPlusIcon,
  GlobeIcon,
  LinkIcon,
  Loader2Icon,
  MoreHorizontalIcon,
  PlusIcon,
  Share2Icon,
  Trash2Icon,
  UnlinkIcon,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import type { ShareInfo } from "@/api/gen/ShareInfo";
import type { SiteInfo } from "@/api/gen/SiteInfo";
import {
  usePhp,
  useProjects,
  useProxies,
  useProxyActions,
  useShareActions,
  useShares,
  useSiteActions,
  useSites,
  useStatus,
} from "@/api/queries";
import { Empty, ErrorNote, PageHeader } from "@/components/common";
import { NewProject } from "@/components/NewProject";
import { ShareDialog } from "@/components/ShareDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

async function chooseFolder(): Promise<string | null> {
  const picked = await pickFolder({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

const host = (url: string) => url.replace(/^https?:\/\//, "");

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success("Copied", { description: text });
  } catch {
    toast.error("Could not copy to the clipboard");
  }
}

export function SitesView() {
  // `undefined`: closed; `null`: the form; a number: that creation's progress.
  const [creating, setCreating] = useState<number | null | undefined>(undefined);
  const projects = useProjects();
  const running = projects.data?.filter((j) => j.state === "running") ?? [];
  const sites = useSites();
  const actions = useSiteActions();
  const pending = Object.values(actions).some((a) => a.isPending);
  const failed = Object.values(actions).find((a) => a.error);

  const park = async () => {
    const path = await chooseFolder();
    if (path) actions.park.mutate(path, { onSuccess: () => toast.success(`Parked ${path}`) });
  };
  const link = async () => {
    const path = await chooseFolder();
    if (path) actions.link.mutate(path, { onSuccess: () => toast.success(`Linked ${path}`) });
  };

  return (
    <div className="space-y-6">
      <PageHeader
        title="Sites"
        description="Every project RED serves, at its own address with the PHP version you pick."
        actions={
          <>
            <Button variant="outline" onClick={park} disabled={pending}>
              <FolderPlusIcon /> Park folder…
            </Button>
            <Button variant="outline" onClick={link} disabled={pending}>
              <LinkIcon /> Link folder…
            </Button>
            {creating === undefined && (
              <Button onClick={() => setCreating(null)}>
                <PlusIcon /> New site…
              </Button>
            )}
          </>
        }
      />
      {creating !== undefined && (
        <NewProject jobId={creating} onClose={() => setCreating(undefined)} />
      )}
      {creating === undefined &&
        running.map((job) => (
          <Card key={job.id} className="flex-row items-center justify-between gap-3 px-6 py-3">
            <span className="flex items-center gap-2 text-sm">
              <Loader2Icon className="size-4 animate-spin text-muted-foreground" />
              Creating <strong>{job.name}</strong>: {job.step}
            </span>
            <Button variant="outline" size="sm" onClick={() => setCreating(job.id)}>
              Show
            </Button>
          </Card>
        ))}
      <ErrorNote error={failed?.error ?? sites.error} />
      <Card className="py-0">
        {sites.data?.length === 0 ? (
          <CardContent className="py-6">
            <Empty icon={<GlobeIcon />}>
              <p>
                No sites yet. Create one with <strong>New site</strong>, park a folder that holds
                your projects, or link a single project.
              </p>
            </Empty>
          </CardContent>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-6">Site</TableHead>
                <TableHead>Driver</TableHead>
                <TableHead>PHP</TableHead>
                <TableHead>HTTPS</TableHead>
                <TableHead className="w-12 pr-6" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {sites.data?.map((site) => (
                <SiteRow key={site.name} site={site} />
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      <ProxiesCard />
    </div>
  );
}

function SiteRow({ site }: { site: SiteInfo }) {
  const actions = useSiteActions();
  const php = usePhp();
  const shares = useShares();
  const shareActions = useShareActions();
  const [sharing, setSharing] = useState(false);
  const versions = php.data?.installs.map((i) => i.version) ?? [];
  // An isolated version that is no longer installed still shows up, so the
  // select never renders empty.
  const missing = site.isolated && site.php && !versions.includes(site.php) ? site.php : null;
  const share = shares.data?.shares.find((s) => s.site === site.name);
  const DEFAULT = "default";

  return (
    <TableRow className="align-top">
      <TableCell className="py-3 pl-6">
        <button
          type="button"
          className="font-medium text-primary hover:underline"
          onClick={() => openUrl(site.url)}
        >
          {host(site.url)}
        </button>
        <div className="max-w-xs truncate text-xs text-muted-foreground" title={site.path}>
          {site.path}
        </div>
        {share && <ShareLine share={share} onStop={() => shareActions.stop.mutate(site.name)} />}
      </TableCell>
      <TableCell className="py-3">
        <Badge variant="secondary">{site.driver}</Badge>
      </TableCell>
      <TableCell className="py-3">
        <Select
          value={site.isolated && site.php ? site.php : DEFAULT}
          onValueChange={(v) =>
            actions.isolate.mutate({ name: site.name, version: v === DEFAULT ? null : v })
          }
        >
          <SelectTrigger size="sm" className="w-36" aria-label={`PHP version for ${site.name}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT}>
              Default{!site.isolated && site.php ? ` (${site.php})` : ""}
            </SelectItem>
            {versions.map((v) => (
              <SelectItem key={v} value={v}>
                PHP {v}
              </SelectItem>
            ))}
            {missing && <SelectItem value={missing}>PHP {missing} (not installed)</SelectItem>}
          </SelectContent>
        </Select>
      </TableCell>
      <TableCell className="py-3">
        <Switch
          checked={site.secure}
          aria-label={`HTTPS for ${site.name}`}
          onCheckedChange={(on) => actions.secure.mutate({ name: site.name, on })}
        />
      </TableCell>
      <TableCell className="py-3 pr-6 text-right">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${site.name}`}>
              <MoreHorizontalIcon />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-52">
            <DropdownMenuItem onSelect={() => openUrl(site.url)}>
              <ExternalLinkIcon /> Open in browser
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => revealItemInDir(site.path)}>
              <FolderOpenIcon /> Show folder
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => copy(site.url)}>
              <CopyIcon /> Copy URL
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {share ? (
              <DropdownMenuItem onSelect={() => shareActions.stop.mutate(site.name)}>
                <Share2Icon /> Stop sharing
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem onSelect={() => setSharing(true)}>
                <Share2Icon /> Share publicly…
              </DropdownMenuItem>
            )}
            {site.kind === "linked" && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() =>
                    actions.unlink.mutate(site.name, {
                      onSuccess: () => toast.success(`Unlinked ${site.name}`),
                    })
                  }
                >
                  <UnlinkIcon /> Unlink
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
        <ShareDialog site={site} open={sharing} onOpenChange={setSharing} />
      </TableCell>
    </TableRow>
  );
}

function ShareLine({ share, onStop }: { share: ShareInfo; onStop: () => void }) {
  if (share.state === "failed") {
    return (
      <div className="mt-1.5 flex items-center gap-2 text-xs text-destructive">
        <Share2Icon className="size-3.5" />
        <span className="truncate">Sharing failed: {share.error}</span>
        <Button variant="link" size="xs" className="h-auto p-0" onClick={onStop}>
          Dismiss
        </Button>
      </div>
    );
  }
  if (share.state === "starting" || !share.url) {
    return (
      <div className="mt-1.5 flex items-center gap-2 text-xs text-muted-foreground">
        <Loader2Icon className="size-3.5 animate-spin" />
        {share.step}…
      </div>
    );
  }
  const url = share.url;
  return (
    <div className="mt-1.5 flex items-center gap-1.5 text-xs">
      <Badge variant="success">
        <Share2Icon /> Shared
      </Badge>
      <button
        type="button"
        className="truncate text-muted-foreground hover:text-foreground hover:underline"
        onClick={() => openUrl(url)}
      >
        {host(url)}
      </button>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Copy public URL"
            onClick={() => copy(url)}
          >
            <CopyIcon />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Copy public URL</TooltipContent>
      </Tooltip>
    </div>
  );
}

function ProxiesCard() {
  const proxies = useProxies();
  const actions = useProxyActions();
  const tld = useStatus().data?.tld ?? "test";
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
    <Card>
      <CardHeader>
        <CardTitle>Proxies</CardTitle>
        <CardDescription>
          Forward a <code>.{tld}</code> name to a local dev server, WebSockets included (Vite,
          Next.js, Reverb).
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <ErrorNote error={actions.add.error ?? actions.remove.error ?? actions.secure.error} />
        <form onSubmit={submit} className="flex flex-wrap gap-2">
          <Input
            aria-label="Proxy name"
            placeholder="name, e.g. app"
            className="w-48"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
          <Input
            aria-label="Proxy target"
            placeholder="port or URL, e.g. 5173"
            className="w-56"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            required
          />
          <Button type="submit" disabled={actions.add.isPending}>
            Add proxy
          </Button>
        </form>
        {proxies.data && proxies.data.length > 0 && (
          <ul className="divide-y rounded-md border text-sm">
            {proxies.data.map((proxy) => (
              <li key={proxy.name} className="flex items-center justify-between gap-3 px-3 py-2">
                <div className="min-w-0">
                  <button
                    type="button"
                    className="font-medium text-primary hover:underline"
                    onClick={() => openUrl(proxy.url)}
                  >
                    {host(proxy.url)}
                  </button>
                  <span className="ml-2 text-xs text-muted-foreground">→ {proxy.target}</span>
                </div>
                <div className="flex items-center gap-3">
                  <Switch
                    checked={proxy.secure}
                    aria-label={`HTTPS for ${proxy.name}`}
                    onCheckedChange={(on) => actions.secure.mutate({ name: proxy.name, on })}
                  />
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Remove proxy ${proxy.name}`}
                    onClick={() => actions.remove.mutate(proxy.name)}
                  >
                    <Trash2Icon />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
