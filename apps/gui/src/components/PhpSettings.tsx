import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api } from "@/api/client";
import type { PhpInstallInfo } from "@/api/gen/PhpInstallInfo";
import type { PhpVersionInfo } from "@/api/gen/PhpVersionInfo";
import { keys } from "@/api/queries";
import { ErrorNote } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

/** The settings the form has fields for (mirrors `php_ini::COMMON`). */
export const COMMON: { key: string; help: string }[] = [
  { key: "memory_limit", help: "Memory one request may use, e.g. 512M or -1" },
  { key: "upload_max_filesize", help: "Largest uploaded file, e.g. 64M" },
  { key: "post_max_size", help: "Largest request body; at least the upload size" },
  { key: "max_execution_time", help: "Seconds a request may run (0: no limit)" },
  { key: "display_errors", help: "Show errors in the page: On or Off" },
  { key: "date.timezone", help: "Default time zone, e.g. Asia/Jakarta" },
];

/** `key = value` lines → settings; blank lines and `;` comments are skipped. */
export function parseExtra(text: string): { settings: Record<string, string>; bad: string[] } {
  const settings: Record<string, string> = {};
  const bad: string[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith(";") || line.startsWith("#")) continue;
    const at = line.indexOf("=");
    if (at <= 0) {
      bad.push(line);
      continue;
    }
    settings[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  return { settings, bad };
}

/** RED's php.ini settings for one PHP version, and what PHP reports. */
export function PhpSettings({ install }: { install: PhpInstallInfo }) {
  const version = install.version;
  const client = useQueryClient();
  const info = useQuery({
    queryKey: [...keys.php, version, "info"],
    queryFn: () => api<PhpVersionInfo>("GET", `/v1/php/${encodeURIComponent(version)}/info`),
  });
  const commonKeys = COMMON.map((c) => c.key);
  const [fields, setFields] = useState<Record<string, string>>(() =>
    Object.fromEntries(commonKeys.map((k) => [k, install.settings[k] ?? ""])),
  );
  const [extra, setExtra] = useState(() =>
    Object.entries(install.settings)
      .filter(([k]) => !commonKeys.includes(k))
      .map(([k, v]) => `${k} = ${v}`)
      .join("\n"),
  );
  const parsed = parseExtra(extra);
  const save = useMutation({
    mutationFn: () =>
      api("PUT", `/v1/php/${encodeURIComponent(version)}/settings`, {
        settings: { ...parsed.settings, ...fields },
      }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: keys.php });
      toast.success(`PHP ${version} settings saved`, {
        description: "Its PHP-FPM pool restarted.",
      });
    },
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (parsed.bad.length === 0) save.mutate();
  };

  return (
    <form onSubmit={submit} className="mt-3 space-y-4 rounded-lg border bg-muted/40 p-4">
      <p className="text-xs text-muted-foreground">
        RED adds these to PHP {version}'s own php.ini
        {info.data?.ini_file ? (
          <>
            {" "}
            (<code>{info.data.ini_file}</code>)
          </>
        ) : null}{" "}
        for sites, <code>red php</code> and <code>red composer</code>. Leave a field empty to keep
        PHP's value.
      </p>
      <div className="grid gap-3 sm:grid-cols-2">
        {COMMON.map(({ key, help }) => (
          <label key={key} className="block space-y-1 text-sm">
            <span className="font-mono text-xs font-medium">{key}</span>
            <Input
              value={fields[key] ?? ""}
              onChange={(e) => setFields({ ...fields, [key]: e.target.value })}
              placeholder={info.data?.values[key] || "(not set)"}
              aria-label={key}
              className="h-8 bg-background font-mono"
            />
            <span className="block text-xs text-muted-foreground">{help}</span>
          </label>
        ))}
      </div>
      <label className="block space-y-1 text-sm">
        <span className="font-medium">Other settings</span>
        <Textarea
          value={extra}
          onChange={(e) => setExtra(e.target.value)}
          rows={3}
          placeholder={"opcache.enable = 0\nerror_reporting = E_ALL"}
          aria-label="Other settings"
          className="bg-background font-mono text-xs"
        />
        {parsed.bad.length > 0 && (
          <span role="alert" className="block text-xs text-destructive">
            Use key = value: {parsed.bad.join(", ")}
          </span>
        )}
      </label>
      <ErrorNote error={save.error ?? info.error} />
      <div className="flex items-center justify-end gap-3">
        {save.isSuccess && !save.isPending && (
          <span className="text-xs text-success">Saved; PHP {version} restarted.</span>
        )}
        <Button type="submit" size="sm" disabled={save.isPending}>
          Save
        </Button>
      </div>
      {info.data && (
        <details className="text-sm">
          <summary className="cursor-pointer text-muted-foreground">
            {info.data.extensions.length} extensions loaded
          </summary>
          <div className="mt-2 flex flex-wrap gap-1">
            {info.data.extensions.map((ext) => (
              <Badge key={ext} variant="secondary">
                {ext}
              </Badge>
            ))}
          </div>
        </details>
      )}
    </form>
  );
}
