import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArchiveIcon,
  ArchiveRestoreIcon,
  DatabaseIcon,
  FolderOpenIcon,
  MoreHorizontalIcon,
  PlusIcon,
  Trash2Icon,
} from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import type { BackupInfo } from "@/api/gen/BackupInfo";
import { useDatabaseActions, useDatabases, useServices } from "@/api/queries";
import { Empty, ErrorNote, formatSize, formatTime, PageHeader } from "@/components/common";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

export function DatabasesView() {
  const services = useServices();
  const candidates = (services.data?.services ?? []).filter(
    (s) => s.kind === "mysql" || s.kind === "postgres",
  );
  const [chosen, setChosen] = useState<string | null>(null);
  const service = candidates.find((s) => s.name === chosen)?.name ?? candidates[0]?.name ?? null;

  return (
    <div className="space-y-6">
      <PageHeader
        title="Databases"
        description="Databases in your MySQL, MariaDB and PostgreSQL services, with backups."
        actions={
          candidates.length > 1 &&
          service && (
            <Select value={service} onValueChange={setChosen}>
              <SelectTrigger className="w-56" aria-label="Service">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {candidates.map((s) => (
                  <SelectItem key={s.name} value={s.name}>
                    {s.name} ({s.flavor ?? s.kind} {s.version})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )
        }
      />
      <ErrorNote error={services.error} />
      {services.data && candidates.length === 0 && (
        <Empty icon={<DatabaseIcon />}>
          <p>No database service yet. Add MySQL, MariaDB or PostgreSQL on the Services page.</p>
        </Empty>
      )}
      {service && <ServiceDatabases key={service} service={service} />}
    </div>
  );
}

function ServiceDatabases({ service }: { service: string }) {
  const databases = useDatabases(service);
  const actions = useDatabaseActions(service);
  const [name, setName] = useState("");
  const [dropping, setDropping] = useState<string | null>(null);
  const [restoring, setRestoring] = useState<{ name: string; file?: string } | null>(null);
  const data = databases.data;
  const failed = Object.values(actions).find((a) => a.error)?.error;

  const create = (event: FormEvent) => {
    event.preventDefault();
    actions.create.mutate(name.trim(), {
      onSuccess: () => {
        toast.success(`Created ${name.trim()}`);
        setName("");
      },
    });
  };
  const backup = (db: string) =>
    actions.backup.mutate(db, {
      onSuccess: (info: BackupInfo) =>
        toast.success(`Backed up ${db}`, {
          description: `${formatSize(info.size)} · ${info.file}`,
          action: { label: "Show", onClick: () => revealItemInDir(info.file) },
        }),
    });

  return (
    <>
      <ErrorNote error={databases.error ?? failed} />
      <Card className="py-0">
        <CardHeader className="border-b py-4">
          <CardTitle>{service}</CardTitle>
          <CardDescription>
            Created databases are empty; restore a dump to fill one.
          </CardDescription>
        </CardHeader>
        <CardContent className="px-0">
          {data && data.databases.length === 0 ? (
            <div className="p-6">
              <Empty icon={<DatabaseIcon />}>
                <p>No databases yet.</p>
              </Empty>
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="pl-6">Database</TableHead>
                  <TableHead>Size</TableHead>
                  <TableHead className="w-12 pr-6" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {data?.databases.map((db) => (
                  <TableRow key={db.name}>
                    <TableCell className="pl-6 font-mono">{db.name}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {db.size === null ? "–" : formatSize(db.size)}
                    </TableCell>
                    <TableCell className="pr-6 text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label={`Actions for ${db.name}`}
                          >
                            <MoreHorizontalIcon />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-48">
                          <DropdownMenuItem onSelect={() => backup(db.name)}>
                            <ArchiveIcon /> Back up
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => setRestoring({ name: db.name })}>
                            <ArchiveRestoreIcon /> Restore a dump…
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            variant="destructive"
                            onSelect={() => setDropping(db.name)}
                          >
                            <Trash2Icon /> Drop…
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
        <form onSubmit={create} className="flex items-center gap-2 border-t px-6 py-4">
          <Input
            aria-label="New database name"
            placeholder="new_database"
            className="w-64 font-mono"
            value={name}
            onChange={(e) => setName(e.target.value.replace(/[^A-Za-z0-9_-]/g, ""))}
            required
          />
          <Button type="submit" variant="outline" disabled={actions.create.isPending || !name}>
            <PlusIcon /> Create database
          </Button>
        </form>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Backups</CardTitle>
          <CardDescription>Gzipped SQL dumps in RED&apos;s data directory.</CardDescription>
        </CardHeader>
        <CardContent>
          {data && data.backups.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No backups yet. Back up a database from its menu.
            </p>
          ) : (
            <ul className="divide-y rounded-md border text-sm">
              {data?.backups.map((b) => (
                <li key={b.file} className="flex items-center justify-between gap-3 px-3 py-2">
                  <div className="min-w-0">
                    <div className="font-mono">{b.database}</div>
                    <div className="text-xs text-muted-foreground">
                      {formatTime(b.created)} · {formatSize(b.size)}
                    </div>
                  </div>
                  <div className="flex shrink-0 gap-1">
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Show ${b.file}`}
                      onClick={() => revealItemInDir(b.file)}
                    >
                      <FolderOpenIcon />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setRestoring({ name: b.database, file: b.file })}
                    >
                      <ArchiveRestoreIcon /> Restore…
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <DropDialog
        name={dropping}
        onClose={() => setDropping(null)}
        onDrop={(db) =>
          actions.drop.mutate(db, {
            onSuccess: () => {
              toast.success(`Dropped ${db}`);
              setDropping(null);
            },
          })
        }
      />
      {restoring && (
        <RestoreDialog
          initial={restoring}
          pending={actions.restore.isPending}
          error={actions.restore.error}
          onClose={() => setRestoring(null)}
          onRestore={(target, file) =>
            actions.restore.mutate(
              { name: target, file },
              {
                onSuccess: () => {
                  toast.success(`Restored into ${target}`);
                  setRestoring(null);
                },
              },
            )
          }
        />
      )}
    </>
  );
}

function DropDialog({
  name,
  onClose,
  onDrop,
}: {
  name: string | null;
  onClose: () => void;
  onDrop: (name: string) => void;
}) {
  const [typed, setTyped] = useState("");
  return (
    <AlertDialog
      open={name !== null}
      onOpenChange={(open) => {
        if (!open) {
          setTyped("");
          onClose();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Drop {name}?</AlertDialogTitle>
          <AlertDialogDescription>
            This deletes the database and all its data. Type its name to confirm.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Input
          aria-label="Database name to confirm"
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          className="font-mono"
        />
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <Button
            variant="destructive"
            disabled={typed !== name}
            onClick={() => name && onDrop(name)}
          >
            Drop database
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

function RestoreDialog({
  initial,
  pending,
  error,
  onClose,
  onRestore,
}: {
  initial: { name: string; file?: string };
  pending: boolean;
  error: unknown;
  onClose: () => void;
  onRestore: (target: string, file: string) => void;
}) {
  const [target, setTarget] = useState(initial.name);
  const [file, setFile] = useState(initial.file ?? "");
  const choose = async () => {
    const picked = await pickFile({
      multiple: false,
      filters: [{ name: "SQL dumps", extensions: ["sql", "gz"] }],
    });
    if (typeof picked === "string") setFile(picked);
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            onRestore(target, file);
          }}
          className="space-y-5"
        >
          <DialogHeader>
            <DialogTitle>Restore a dump</DialogTitle>
            <DialogDescription>
              Loads a .sql or .sql.gz file into a database, creating it if it does not exist.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <Label htmlFor="restore-file">Dump</Label>
            <div className="flex gap-2">
              <Input
                id="restore-file"
                value={file}
                onChange={(e) => setFile(e.target.value)}
                placeholder="/path/to/dump.sql.gz"
                className="font-mono text-xs"
                required
              />
              <Button type="button" variant="outline" onClick={choose}>
                Choose…
              </Button>
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="restore-target">Into database</Label>
            <Input
              id="restore-target"
              value={target}
              onChange={(e) => setTarget(e.target.value.replace(/[^A-Za-z0-9_-]/g, ""))}
              className="font-mono"
              required
            />
          </div>
          <ErrorNote error={error} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending || !file || !target}>
              {pending ? "Restoring…" : "Restore"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
