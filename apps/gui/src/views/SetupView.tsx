import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { CheckIcon } from "lucide-react";
import { type ReactNode, useState } from "react";
import { ErrorNote } from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { api, RedApiError } from "../api/client";
import type { Check } from "../api/gen/Check";
import type { StatusResponse } from "../api/gen/StatusResponse";
import { keys, useDoctor } from "../api/queries";
import { RuntimeInstaller } from "../components/RuntimeInstaller";

const DONE_KEY = "red.setup.done";

/** Doctor checks the setup steps fix. */
export const SETUP_CHECKS = ["helper", "autostart", "php", "resolver", "ca"] as const;

/** Whether the user finished or skipped setup before (per machine). */
export function setupDismissed(): boolean {
  try {
    return localStorage.getItem(DONE_KEY) === "1";
  } catch {
    return false;
  }
}

function dismissSetup() {
  try {
    localStorage.setItem(DONE_KEY, "1");
  } catch {
    // Without storage the screen simply shows again next time.
  }
}

/** `true` when some setup step still needs doing. */
export function needsSetup(checks: Check[] | undefined): boolean {
  if (!checks) return false;
  return SETUP_CHECKS.some((id) => {
    const check = checks.find((c) => c.id === id);
    return check !== undefined && check.status !== "ok";
  });
}

type CliStatus = { bundled: string | null; installed: string | null };

function toError(err: unknown): RedApiError {
  if (err instanceof RedApiError) return err;
  if (typeof err === "object" && err !== null && "message" in err) {
    return new RedApiError(err as ConstructorParameters<typeof RedApiError>[0]);
  }
  return new RedApiError({ code: "internal", message: String(err) });
}

/** A step the Tauri shell runs (`setup_step`): `system`, `autostart`, `trust`. */
function useStep(step: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("setup_step", { step });
      } catch (err) {
        throw toError(err);
      }
    },
    onSettled: () => {
      client.invalidateQueries({ queryKey: keys.doctor });
      client.invalidateQueries({ queryKey: keys.status });
    },
  });
}

/** The user closing the password dialog is not an error worth showing. */
function shown(error: unknown) {
  return error instanceof RedApiError && error.code === "cancelled" ? null : error;
}

/**
 * First-run setup: every step `red setup` does, one button each, in order.
 * Steps that are already in place show as done.
 */
export function SetupView({ status, onClose }: { status: StatusResponse; onClose: () => void }) {
  const doctor = useDoctor();
  const client = useQueryClient();
  const checks = doctor.data?.checks ?? [];
  const find = (id: string) => checks.find((c) => c.id === id);
  const ok = (id: string) => find(id)?.status === "ok";

  const cli = useQuery({ queryKey: ["cli"], queryFn: () => invoke<CliStatus>("cli_status") });
  const installCli = useMutation({
    mutationFn: async () => {
      try {
        return await invoke<string>("install_cli");
      } catch (err) {
        throw toError(err);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: ["cli"] }),
  });
  const system = useStep("system");
  const autostart = useStep("autostart");
  const trust = useStep("trust");
  const dns = useMutation({
    mutationFn: () => api("POST", "/v1/dns"),
    onSettled: () => client.invalidateQueries({ queryKey: keys.doctor }),
  });
  const [showPhp, setShowPhp] = useState(false);

  const helper = find("helper");
  // Installed but redd started before it: the next step's restart fixes it.
  const helperInstalled = ok("helper") || Boolean(helper?.detail.startsWith("installed"));
  const linux = status.os === "linux";

  const steps: StepProps[] = [
    {
      title: "Serve sites on ports 80 and 443",
      text: linux
        ? "Installs RED's small privileged helper and adds RED's certificate authority to the system. Asks for your password once."
        : "Installs RED's small privileged helper. macOS asks for your password once.",
      done: helperInstalled,
      busy: system.isPending,
      error: shown(system.error),
      action: (
        <Button onClick={() => system.mutate()} disabled={system.isPending}>
          {system.isPending ? "Waiting for password…" : "Install helper"}
        </Button>
      ),
    },
    {
      title: "Start RED at login",
      text: "RED runs in the background, so sites keep working after you close this window.",
      done: ok("autostart") && ok("helper"),
      busy: autostart.isPending,
      error: shown(autostart.error),
      action: (
        <Button
          onClick={() => autostart.mutate()}
          disabled={autostart.isPending || !helperInstalled}
        >
          {autostart.isPending ? "Restarting RED…" : "Start at login"}
        </Button>
      ),
    },
    {
      title: "Install PHP",
      text:
        find("php")?.status === "ok" ? find("php")?.detail : "RED's own PHP build, with Xdebug.",
      done: ok("php"),
      action: (
        <Button onClick={() => setShowPhp((v) => !v)}>
          {showPhp ? "Hide versions" : "Choose a version"}
        </Button>
      ),
      extra: showPhp && !ok("php") ? <RuntimeInstaller kinds={["php"]} /> : null,
    },
    {
      title: `Use .${status.tld} domains`,
      text: `Sends *.${status.tld} to this machine, so every site gets its own address.`,
      done: ok("resolver"),
      busy: dns.isPending,
      error: dns.error,
      action: (
        <Button onClick={() => dns.mutate()} disabled={dns.isPending || !ok("helper")}>
          Set up DNS
        </Button>
      ),
    },
    {
      title: "Trust HTTPS certificates",
      text: linux
        ? "Adds RED's certificate authority to Chrome's and Firefox's stores."
        : "Adds RED's certificate authority to your login keychain. macOS asks to confirm.",
      done: ok("ca"),
      busy: trust.isPending,
      error: shown(trust.error),
      action: (
        <Button onClick={() => trust.mutate()} disabled={trust.isPending}>
          Trust certificates
        </Button>
      ),
    },
    {
      title: "Use red in your terminal",
      text: cli.data?.installed
        ? `Installed at ${cli.data.installed}.`
        : "Adds the red command to ~/.local/bin, for red park, red php, red mcp and more.",
      done: Boolean(cli.data?.installed),
      busy: installCli.isPending,
      error: installCli.error,
      action: cli.data?.bundled ? (
        <Button onClick={() => installCli.mutate()} disabled={installCli.isPending}>
          Install command
        </Button>
      ) : null,
    },
  ];
  const done = steps.filter((s) => s.done).length;
  const finish = () => {
    dismissSetup();
    onClose();
  };

  return (
    <div className="mx-auto max-w-2xl space-y-6">
      <header className="space-y-1">
        <h1 className="text-xl font-semibold">Set up RED</h1>
        <p className="text-sm text-muted-foreground">
          A few one-time steps so sites work at https://name.{status.tld}. {done} of {steps.length}{" "}
          done.
        </p>
      </header>
      {doctor.error && <ErrorNote error={doctor.error} />}
      <ol className="space-y-3">
        {steps.map((step, index) => (
          <Step key={step.title} number={index + 1} {...step} />
        ))}
      </ol>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={() => doctor.refetch()} disabled={doctor.isFetching}>
          Check again
        </Button>
        {done < steps.length && (
          <Button variant="outline" onClick={finish}>
            Skip for now
          </Button>
        )}
        <Button onClick={finish} disabled={done < steps.length}>
          Done
        </Button>
      </div>
    </div>
  );
}

type StepProps = {
  title: string;
  text?: string;
  done: boolean;
  busy?: boolean;
  error?: unknown;
  action: ReactNode;
  extra?: ReactNode;
};

function Step({
  number,
  title,
  text,
  done,
  busy,
  error,
  action,
  extra,
}: StepProps & { number: number }) {
  return (
    <li aria-label={title}>
      <Card className="gap-0 py-4">
        <CardContent className="px-4">
          <div className="flex items-start gap-3">
            <span
              aria-hidden
              className={cn(
                "grid size-6 shrink-0 place-items-center rounded-full text-xs font-semibold",
                done ? "bg-success text-white" : "bg-muted text-muted-foreground",
              )}
            >
              {done ? <CheckIcon className="size-3.5" /> : number}
            </span>
            <div className="min-w-0 flex-1 space-y-1">
              <h2 className="font-medium">{title}</h2>
              {text && <p className="text-sm text-muted-foreground">{text}</p>}
              <ErrorNote error={error} />
            </div>
            <div className="shrink-0" aria-busy={busy || undefined}>
              {done ? <Badge variant="success">Done</Badge> : action}
            </div>
          </div>
          {extra && <div className="mt-3 pl-9">{extra}</div>}
        </CardContent>
      </Card>
    </li>
  );
}
