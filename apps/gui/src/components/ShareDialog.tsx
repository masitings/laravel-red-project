import { openUrl } from "@tauri-apps/plugin-opener";
import { CloudIcon, GlobeLockIcon, Loader2Icon } from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import type { SiteInfo } from "@/api/gen/SiteInfo";
import { useShareActions, useShares } from "@/api/queries";
import { ErrorNote } from "@/components/common";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

type Mode = "quick" | "domain";

/**
 * Share a site through Cloudflare Tunnel: a one-off trycloudflare.com link,
 * or a hostname on the user's own Cloudflare account.
 */
export function ShareDialog({
  site,
  open,
  onOpenChange,
}: {
  site: SiteInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const shares = useShares();
  const actions = useShareActions();
  const [mode, setMode] = useState<Mode>("quick");
  const [hostname, setHostname] = useState("");
  const loggedIn = shares.data?.logged_in ?? false;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    actions.start.mutate(
      { site: site.name, hostname: mode === "domain" ? hostname.trim() : undefined },
      {
        onSuccess: () => {
          toast.info(`Sharing ${site.name}…`, {
            description: "The public URL shows up in the list once Cloudflare connects.",
          });
          onOpenChange(false);
        },
      },
    );
  };
  const login = () =>
    actions.login.mutate(undefined, {
      onSuccess: (response) => {
        if (response.url) openUrl(response.url);
      },
    });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={submit} className="space-y-5">
          <DialogHeader>
            <DialogTitle>Share {site.name} on the internet</DialogTitle>
            <DialogDescription>
              Cloudflare Tunnel forwards visitors to this site: no open ports, no router setup.
              Anyone with the link can open it until you stop sharing.
            </DialogDescription>
          </DialogHeader>
          <ToggleGroup
            type="single"
            variant="outline"
            value={mode}
            onValueChange={(v) => v && setMode(v as Mode)}
            className="w-full"
          >
            <ToggleGroupItem value="quick" className="flex-1">
              <CloudIcon /> One-off link
            </ToggleGroupItem>
            <ToggleGroupItem value="domain" className="flex-1">
              <GlobeLockIcon /> My domain
            </ToggleGroupItem>
          </ToggleGroup>
          {mode === "quick" ? (
            <p className="text-sm text-muted-foreground">
              A random <code>https://…trycloudflare.com</code> address, no account needed. It
              changes every time you share.
            </p>
          ) : loggedIn ? (
            <div className="space-y-2">
              <Label htmlFor="share-hostname">Hostname</Label>
              <Input
                id="share-hostname"
                placeholder="app.example.com"
                value={hostname}
                onChange={(e) => setHostname(e.target.value.toLowerCase())}
                required
              />
              <p className="text-xs text-muted-foreground">
                A name on a domain in your Cloudflare account. RED creates the tunnel and the DNS
                record; it stays the same every time.
              </p>
            </div>
          ) : (
            <div className="space-y-3 rounded-lg border bg-muted/40 p-3 text-sm">
              <p>
                Log cloudflared in to your Cloudflare account once, pick the domain, then come back
                here.
              </p>
              <Button
                type="button"
                variant="outline"
                onClick={login}
                disabled={actions.login.isPending}
              >
                {actions.login.isPending && <Loader2Icon className="animate-spin" />}
                Log in to Cloudflare
              </Button>
              <ErrorNote error={actions.login.error} />
            </div>
          )}
          <ErrorNote error={actions.start.error} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={actions.start.isPending || (mode === "domain" && (!loggedIn || !hostname))}
            >
              Share
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
