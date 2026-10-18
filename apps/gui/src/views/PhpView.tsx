import { usePhp, useSetDefaultPhp } from "../api/queries";
import { RuntimeInstaller } from "../components/RuntimeInstaller";
import { Badge, Button, Card, ErrorNote } from "../components/ui";

export function PhpView() {
  return (
    <div className="space-y-6">
      <Versions />
      <Card title="Install PHP">
        <p className="mb-3 text-sm text-stone-500">
          RED downloads self-contained PHP builds (with the common extensions) from static-php-cli.
          No Homebrew needed.
        </p>
        <RuntimeInstaller kinds={["php"]} />
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
