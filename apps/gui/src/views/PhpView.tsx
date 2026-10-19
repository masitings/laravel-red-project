import type { XdebugMode } from "../api/gen/XdebugMode";
import { usePhp, useSetDefaultPhp, useSetXdebug } from "../api/queries";
import { RuntimeInstaller } from "../components/RuntimeInstaller";
import { Badge, Button, Card, ErrorNote } from "../components/ui";

export function PhpView() {
  return (
    <div className="space-y-6">
      <Versions />
      <Xdebug />
      <Card title="Install PHP">
        <p className="mb-3 text-sm text-stone-500">
          RED downloads self-contained PHP builds, no Homebrew needed: its own (with pdo_sqlite,
          pdo_pgsql, intl, redis, imagick and more) and static-php-cli's for versions it has not
          built yet.
        </p>
        <RuntimeInstaller kinds={["php"]} />
      </Card>
      <Card title="Composer and Node.js">
        <p className="mb-3 text-sm text-stone-500">
          <code>red composer</code>, <code>red node</code> and <code>red npm</code> use these, and
          install them on first use. A project picks its Node.js version with <code>node</code> in{" "}
          <code>red.toml</code> or <code>.nvmrc</code>.
        </p>
        <RuntimeInstaller kinds={["composer", "node"]} />
      </Card>
    </div>
  );
}

function Versions() {
  const php = usePhp();
  const setDefault = useSetDefaultPhp();
  const installs = php.data?.installs ?? [];

  return (
    <Card title="PHP versions">
      <ErrorNote error={php.error ?? setDefault.error} />
      {php.data && installs.length === 0 && (
        <p className="text-sm text-stone-500">
          No PHP yet. Install one below, or with <code>brew install php</code>.
        </p>
      )}
      <ul className="divide-y divide-stone-100 dark:divide-stone-800">
        {installs.map((install) => {
          const isDefault = php.data?.default === install.version;
          return (
            <li key={install.version} className="flex items-center justify-between gap-3 py-3">
              <div>
                <div className="flex items-center gap-2 font-medium">
                  PHP {install.full_version}
                  {isDefault && <Badge tone="good">default</Badge>}
                  {install.running && <Badge>running</Badge>}
                  {install.xdebug_loaded ? (
                    <Badge tone="warn">Xdebug always on (php.ini)</Badge>
                  ) : (
                    install.xdebug && <Badge>Xdebug</Badge>
                  )}
                </div>
                <div className="text-xs text-stone-500">{install.fpm_path}</div>
              </div>
              {!isDefault && (
                <Button
                  onClick={() => setDefault.mutate(install.version)}
                  disabled={setDefault.isPending}
                >
                  Make default
                </Button>
              )}
            </li>
          );
        })}
      </ul>
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
    <Card title="Xdebug">
      <div className="space-y-3 text-sm">
        <div
          role="radiogroup"
          aria-label="Xdebug mode"
          className="inline-flex rounded-md border border-stone-300 dark:border-stone-700"
        >
          {xdebugModes.map(({ mode, label }) => (
            <button
              key={mode}
              type="button"
              role="radio"
              aria-checked={data.xdebug === mode}
              disabled={setXdebug.isPending}
              onClick={() => setXdebug.mutate(mode)}
              className={`px-3 py-1 text-sm first:rounded-l-md last:rounded-r-md ${
                data.xdebug === mode
                  ? "bg-red-600 font-medium text-white"
                  : "hover:bg-stone-100 dark:hover:bg-stone-800"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
        <p className="text-stone-600 dark:text-stone-400">{help}</p>
        {data.xdebug !== "off" && (
          <p className="text-stone-600 dark:text-stone-400">
            Let your IDE listen for Xdebug on port <code>{data.xdebug_port}</code>.
          </p>
        )}
        {missing.length > 0 && (
          <p className="text-xs text-stone-500">
            No Xdebug for PHP {missing.map((i) => i.version).join(", ")}. RED&apos;s macOS builds
            include it (update them on this page); elsewhere, install your system&apos;s php-xdebug.
          </p>
        )}
        <ErrorNote error={setXdebug.error} />
      </div>
    </Card>
  );
}
