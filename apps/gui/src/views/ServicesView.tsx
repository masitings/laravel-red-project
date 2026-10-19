import {
  MoreHorizontalIcon,
  PencilIcon,
  PlayIcon,
  RotateCwIcon,
  ScrollTextIcon,
  ServerIcon,
  SquareIcon,
  Trash2Icon,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import type { ServiceInfo } from "@/api/gen/ServiceInfo";
import type { ServiceStatus } from "@/api/gen/ServiceStatus";
import { useServiceActions, useServices } from "@/api/queries";
import { CodeBlock, Empty, ErrorNote, PageHeader } from "@/components/common";
import { RuntimeInstaller } from "@/components/RuntimeInstaller";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
import { LogEntries } from "@/views/LogsView";

const statusVariant: Record<ServiceStatus, "success" | "warning" | "destructive" | "secondary"> = {
  running: "success",
  starting: "warning",
  failed: "destructive",
  stopped: "secondary",
};

export function ServicesView() {
  const services = useServices();
  const actions = useServiceActions();
  const failed = Object.values(actions).find((a) => a.error);
  const list = services.data?.services ?? [];

  return (
    <div className="space-y-6">
      <PageHeader
        title="Services"
        description="Databases and caches RED runs on 127.0.0.1 and starts with redd."
      />
      <ErrorNote error={failed?.error ?? services.error} />
      {services.data && list.length === 0 && (
        <Empty icon={<ServerIcon />}>
          <p>No services yet. Add MySQL, MariaDB, PostgreSQL or Valkey below.</p>
        </Empty>
      )}
      <div className="space-y-3">
        {list.map((service) => (
          <ServiceCard key={service.name} service={service} />
        ))}
      </div>
      <AddService />
      <Card>
        <CardHeader>
          <CardTitle>Download a server</CardTitle>
          <CardDescription>
            RED downloads MySQL, MariaDB, PostgreSQL and Valkey (a Redis-compatible cache) itself;
            they then appear under &ldquo;Add a service&rdquo;.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <RuntimeInstaller kinds={["mysql", "mariadb", "postgres", "valkey"]} />
        </CardContent>
      </Card>
    </div>
  );
}

function ServiceCard({ service }: { service: ServiceInfo }) {
  const actions = useServiceActions();
  const [removing, setRemoving] = useState(false);
  const [editing, setEditing] = useState(false);
  const [showLog, setShowLog] = useState(false);
  const running = service.status === "running" || service.status === "starting";
  const server = [service.flavor ?? service.kind, service.version].filter(Boolean).join(" ");
  const act = (action: "start" | "stop" | "restart") =>
    actions.action.mutate({ name: service.name, action });

  return (
    <Card className="gap-4 py-4">
      <CardContent className="space-y-3 px-4">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2 font-medium">
              {service.name}
              <Badge variant={statusVariant[service.status]}>{service.status}</Badge>
            </div>
            <div className="text-xs text-muted-foreground">
              {server} · 127.0.0.1:{service.port}
              {service.autostart && " · starts with redd"}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <Button size="sm" variant="outline" onClick={() => act(running ? "stop" : "start")}>
              {running ? <SquareIcon /> : <PlayIcon />}
              {running ? "Stop" : "Start"}
            </Button>
            {running && (
              <Button size="sm" variant="ghost" onClick={() => act("restart")}>
                <RotateCwIcon /> Restart
              </Button>
            )}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${service.name}`}>
                  <MoreHorizontalIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-48">
                <DropdownMenuItem onSelect={() => setEditing(true)}>
                  <PencilIcon /> Edit…
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setShowLog((v) => !v)}>
                  <ScrollTextIcon /> {showLog ? "Hide log" : "Show log"}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => setRemoving(true)}>
                  <Trash2Icon /> Remove…
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
        {service.status === "failed" && service.error && (
          <ErrorNote error={new Error(`${service.error} (log: ${service.log_file})`)} />
        )}
        <details>
          <summary className="cursor-pointer text-xs text-muted-foreground">.env settings</summary>
          <CodeBlock className="mt-2">
            {service.env.map((v) => `${v.name}=${v.value}`).join("\n")}
          </CodeBlock>
        </details>
        {showLog && (
          <div className="rounded-md border p-3">
            <LogEntries id={`service:${service.name}`} />
          </div>
        )}
      </CardContent>
      <EditService service={service} open={editing} onOpenChange={setEditing} />
      <AlertDialog open={removing} onOpenChange={setRemoving}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {service.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              RED stops the server and forgets the service. Keep its data to add it back later, or
              delete it for good.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <Button
              variant="outline"
              onClick={() => actions.remove.mutate({ name: service.name, deleteData: false })}
            >
              Remove, keep data
            </Button>
            <Button
              variant="destructive"
              onClick={() => actions.remove.mutate({ name: service.name, deleteData: true })}
            >
              Remove and delete data
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

function EditService({
  service,
  open,
  onOpenChange,
}: {
  service: ServiceInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const services = useServices();
  const actions = useServiceActions();
  const [port, setPort] = useState(String(service.port));
  const [version, setVersion] = useState("");
  const [autostart, setAutostart] = useState(service.autostart);
  // Data only moves between versions of the same server (Redis and Valkey
  // share a format).
  const versions = (services.data?.available ?? []).filter(
    (i) =>
      i.kind === service.kind &&
      (service.kind === "redis" || i.flavor === service.flavor) &&
      i.binary !== service.binary,
  );

  const submit = (event: FormEvent) => {
    event.preventDefault();
    actions.update.mutate(
      {
        name: service.name,
        port: Number(port) !== service.port ? Number(port) : undefined,
        version: version || undefined,
        autostart: autostart !== service.autostart ? autostart : undefined,
      },
      {
        onSuccess: () => {
          toast.success(`${service.name} updated`);
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="space-y-5">
          <DialogHeader>
            <DialogTitle>Edit {service.name}</DialogTitle>
            <DialogDescription>
              Changes restart the service. Update the port in your projects&apos; .env afterwards.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <Label htmlFor={`port-${service.name}`}>Port</Label>
            <Input
              id={`port-${service.name}`}
              inputMode="numeric"
              value={port}
              onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
              required
            />
          </div>
          <div className="space-y-2">
            <Label>Server version</Label>
            <Select value={version} onValueChange={setVersion} disabled={versions.length === 0}>
              <SelectTrigger className="w-full" aria-label="Server version">
                <SelectValue
                  placeholder={`${service.flavor ?? service.kind} ${service.version ?? ""} (current)`}
                />
              </SelectTrigger>
              <SelectContent>
                {versions.map((i) => (
                  <SelectItem key={i.binary} value={i.full_version}>
                    {i.flavor} {i.full_version}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {versions.length === 0
                ? "No other version of this server is installed."
                : "Only newer versions of the same server; PostgreSQL stays on its major version."}
            </p>
          </div>
          <div className="flex items-center gap-3">
            <Switch
              id={`autostart-${service.name}`}
              checked={autostart}
              onCheckedChange={setAutostart}
            />
            <Label htmlFor={`autostart-${service.name}`}>Start with redd</Label>
          </div>
          <ErrorNote error={actions.update.error} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={actions.update.isPending}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function AddService() {
  const services = useServices();
  const actions = useServiceActions();
  const available = services.data?.available ?? [];
  const [choice, setChoice] = useState("");
  const [port, setPort] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const install = available[Number(choice)];
    if (!install) return;
    actions.add.mutate(
      {
        kind: install.kind,
        version: install.full_version,
        port: port ? Number(port) : undefined,
      },
      {
        onSuccess: () => {
          setPort("");
          toast.success(`Added ${install.flavor} ${install.full_version}`);
        },
      },
    );
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Add a service</CardTitle>
        <CardDescription>Pick a server RED found or downloaded.</CardDescription>
      </CardHeader>
      <CardContent>
        {services.data && available.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No database servers found yet. Download one below, or install one with Homebrew.
          </p>
        ) : (
          <form onSubmit={submit} className="flex flex-wrap items-center gap-2">
            <Select value={choice} onValueChange={setChoice}>
              <SelectTrigger className="w-64" aria-label="Server">
                <SelectValue placeholder="Choose a server…" />
              </SelectTrigger>
              <SelectContent>
                {available.map((install, index) => (
                  <SelectItem key={install.binary} value={String(index)}>
                    {install.flavor} {install.full_version}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Input
              aria-label="Port"
              placeholder="port (default)"
              inputMode="numeric"
              className="w-36"
              value={port}
              onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
            />
            <Button type="submit" disabled={actions.add.isPending || choice === ""}>
              Add service
            </Button>
          </form>
        )}
      </CardContent>
    </Card>
  );
}
