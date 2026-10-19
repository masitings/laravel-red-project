import { CodeIcon, SlidersHorizontalIcon, StarIcon } from "lucide-react";
import { useState } from "react";
import type { XdebugMode } from "@/api/gen/XdebugMode";
import { usePhp, useSetDefaultPhp, useSetXdebug } from "@/api/queries";
import { CodeBlock, Empty, ErrorNote, PageHeader } from "@/components/common";
import { PhpSettings } from "@/components/PhpSettings";
import { RuntimeInstaller } from "@/components/RuntimeInstaller";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

export function PhpView() {
  return (
    <div className="space-y-6">
      <PageHeader
        title="PHP"
        description="Versions RED found or installed, php.ini settings per version, and Xdebug."
      />
      <Versions />
      <Xdebug />
      <Card>
        <CardHeader>
          <CardTitle>Install PHP</CardTitle>
          <CardDescription>
            Self-contained builds, no Homebrew needed: RED&apos;s own (with pdo_sqlite, pdo_pgsql,
            intl, redis, imagick, Xdebug and more) and static-php-cli&apos;s for versions RED has
            not built yet.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <RuntimeInstaller kinds={["php"]} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Composer, Node.js and Bun</CardTitle>
          <CardDescription>
            <code>red composer</code>, <code>red node</code>, <code>red npm</code> and{" "}
            <code>red bun</code> use these and install them on first use. A project picks its
            Node.js version with <code>node</code> in <code>red.toml</code> or <code>.nvmrc</code>.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <RuntimeInstaller kinds={["composer", "node", "bun"]} />
        </CardContent>
      </Card>
    </div>
  );
}

function Versions() {
  const php = usePhp();
  const setDefault = useSetDefaultPhp();
  const installs = php.data?.installs ?? [];
  const [open, setOpen] = useState<string | null>(null);

  return (
    <Card>
      <CardHeader>
        <CardTitle>PHP versions</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <ErrorNote error={php.error ?? setDefault.error} />
        {php.data && installs.length === 0 && (
          <Empty icon={<CodeIcon />}>
            <p>No PHP yet. Install one below, or with Homebrew.</p>
          </Empty>
        )}
        <ul className="divide-y rounded-md border">
          {installs.map((install) => {
            const isDefault = php.data?.default === install.version;
            const count = Object.keys(install.settings).length;
            return (
              <li key={install.version} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2 font-medium">
                      PHP {install.full_version}
                      {isDefault && <Badge variant="success">default</Badge>}
                      {install.running && <Badge variant="secondary">running</Badge>}
                      {install.xdebug_loaded ? (
                        <Badge variant="warning">Xdebug always on (php.ini)</Badge>
                      ) : (
                        install.xdebug && <Badge variant="outline">Xdebug</Badge>
                      )}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">{install.fpm_path}</div>
                  </div>
                  <div className="flex shrink-0 gap-2">
                    <Button
                      size="sm"
                      variant={open === install.version ? "secondary" : "ghost"}
                      aria-expanded={open === install.version}
                      onClick={() => setOpen(open === install.version ? null : install.version)}
                    >
                      <SlidersHorizontalIcon />
                      Settings
                      {count > 0 && ` (${count})`}
                    </Button>
                    {!isDefault && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setDefault.mutate(install.version)}
                        disabled={setDefault.isPending}
                      >
                        <StarIcon /> Make default
                      </Button>
                    )}
                  </div>
                </div>
                {open === install.version && <PhpSettings install={install} />}
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}

const xdebugModes: { mode: XdebugMode; label: string; help: string }[] = [
  { mode: "off", label: "Off", help: "Sites never run with Xdebug." },
  {
    mode: "auto",
    label: "On demand",
    help: "Requests that carry the XDEBUG_SESSION cookie (set by your IDE or an Xdebug browser extension) run with Xdebug; everything else stays fast. red php runs with it when XDEBUG_TRIGGER=1 is set.",
  },
  {
    mode: "on",
    label: "Always",
    help: "Every request and every red php run starts a debug session. Slower; handy for a quick look.",
  },
];

function Xdebug() {
  const php = usePhp();
  const setXdebug = useSetXdebug();
  const data = php.data;
  if (!data) return null;
  const help = xdebugModes.find((m) => m.mode === data.xdebug)?.help ?? "";
  const missing = data.installs.filter((i) => !i.xdebug && !i.xdebug_loaded);
  return (
    <Card>
      <CardHeader>
        <CardTitle>Xdebug</CardTitle>
        <CardDescription>{help}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <fieldset className="inline-flex rounded-md border p-0.5">
          <legend className="sr-only">Xdebug mode</legend>
          {xdebugModes.map(({ mode, label }) => (
            <label
              key={mode}
              className={cn(
                "cursor-pointer rounded-sm px-3 py-1 text-sm transition-colors has-[:focus-visible]:ring-[3px] has-[:focus-visible]:ring-ring/50",
                data.xdebug === mode
                  ? "bg-primary font-medium text-primary-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
              )}
            >
              <input
                type="radio"
                name="xdebug-mode"
                value={mode}
                className="sr-only"
                checked={data.xdebug === mode}
                disabled={setXdebug.isPending}
                onChange={() => setXdebug.mutate(mode)}
              />
              {label}
            </label>
          ))}
        </fieldset>
        {data.xdebug !== "off" && (
          <p className="text-muted-foreground">
            Let your IDE listen for Xdebug on port <code>{data.xdebug_port}</code>.
          </p>
        )}
        {missing.length > 0 && (
          <p className="text-xs text-muted-foreground">
            No Xdebug for PHP {missing.map((i) => i.version).join(", ")}. RED&apos;s macOS builds
            include it (update them on this page); elsewhere, install your system&apos;s php-xdebug.
          </p>
        )}
        {data.xdebug === "auto" && (
          <details className="text-xs text-muted-foreground">
            <summary className="cursor-pointer">Start a session from the command line</summary>
            <CodeBlock className="mt-2">XDEBUG_TRIGGER=1 red php artisan migrate</CodeBlock>
          </details>
        )}
        <ErrorNote error={setXdebug.error} />
      </CardContent>
    </Card>
  );
}
