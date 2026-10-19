import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api } from "../api/client";
import type { PhpInstallInfo } from "../api/gen/PhpInstallInfo";
import type { PhpVersionInfo } from "../api/gen/PhpVersionInfo";
import { keys } from "../api/queries";
import { Badge, Button, ErrorNote } from "./ui";

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
    },
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (parsed.bad.length === 0) save.mutate();
  };

  return (
    <form onSubmit={submit} className="mt-3 space-y-4 rounded-lg bg-stone-50 p-4 dark:bg-stone-950">
      <p className="text-xs text-stone-500">
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
            <input
              value={fields[key] ?? ""}
              onChange={(e) => setFields({ ...fields, [key]: e.target.value })}
              placeholder={info.data?.values[key] || "(not set)"}
              aria-label={key}
              className="w-full rounded-md border border-stone-300 bg-white px-2 py-1 font-mono dark:border-stone-700 dark:bg-stone-900"
            />
            <span className="block text-xs text-stone-500">{help}</span>
          </label>
        ))}
      </div>
      <label className="block space-y-1 text-sm">
        <span className="font-medium">Other settings</span>
        <textarea
          value={extra}
          onChange={(e) => setExtra(e.target.value)}
          rows={3}
          placeholder={"opcache.enable = 0\nerror_reporting = E_ALL"}
          aria-label="Other settings"
          className="w-full rounded-md border border-stone-300 bg-white px-2 py-1 font-mono text-xs dark:border-stone-700 dark:bg-stone-900"
        />
        {parsed.bad.length > 0 && (
          <span role="alert" className="block text-xs text-red-700 dark:text-red-400">
            Use key = value: {parsed.bad.join(", ")}
          </span>
        )}
      </label>
      <ErrorNote error={save.error ?? info.error} />
      <div className="flex items-center justify-end gap-3">
        {save.isSuccess && !save.isPending && (
          <span className="text-xs text-emerald-700 dark:text-emerald-400">
            Saved; PHP {version} restarted.
          </span>
        )}
        <Button type="submit" variant="primary" disabled={save.isPending}>
          Save
        </Button>
      </div>
      {info.data && (
        <details className="text-sm">
          <summary className="cursor-pointer text-stone-600 dark:text-stone-400">
            {info.data.extensions.length} extensions loaded
          </summary>
          <div className="mt-2 flex flex-wrap gap-1">
            {info.data.extensions.map((ext) => (
              <Badge key={ext}>{ext}</Badge>
            ))}
          </div>
        </details>
      )}
    </form>
  );
}
