import { AlertCircleIcon } from "lucide-react";
import { type ReactNode, useRef } from "react";
import { RedApiError } from "@/api/client";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

/** Title, one line of explanation and actions at the top of a page. */
export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0 space-y-1">
        <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}

/** A failed request, with the daemon's hint when it gave one. */
export function ErrorNote({ error, className }: { error: unknown; className?: string }) {
  if (!error) return null;
  const message = error instanceof Error ? error.message : String(error);
  const hint = error instanceof RedApiError ? error.hint : undefined;
  return (
    <Alert variant="destructive" className={className}>
      <AlertCircleIcon />
      <AlertTitle>{message}</AlertTitle>
      {hint && <AlertDescription>{hint}</AlertDescription>}
    </Alert>
  );
}

/** Text shown where a list has nothing in it yet. */
export function Empty({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed px-6 py-10 text-center text-sm text-muted-foreground [&_svg]:size-8 [&_svg]:text-muted-foreground/60">
      {icon}
      <div className="max-w-md space-y-2">{children}</div>
    </div>
  );
}

/** Copyable lines of text, e.g. `.env` settings. */
export function CodeBlock({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <pre
      className={cn(
        "overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs select-all",
        className,
      )}
    >
      {children}
    </pre>
  );
}

/**
 * Untrusted HTML (mail bodies, rendered dumps) in a sandboxed frame: no
 * scripts, forms or navigation, and the app's CSP blocks remote content.
 * Same-origin is allowed only so the frame can be sized to its content.
 */
export function HtmlFrame({
  html,
  title,
  minHeight = 40,
  fill = false,
  css = "",
}: {
  html: string;
  title: string;
  minHeight?: number;
  fill?: boolean;
  /** Extra CSS for the frame's document, e.g. its background. */
  css?: string;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const fit = () => {
    const doc = frame.current?.contentDocument;
    if (!fill && frame.current && doc?.documentElement) {
      frame.current.style.height = `${Math.max(minHeight, doc.documentElement.scrollHeight)}px`;
    }
  };
  return (
    <iframe
      ref={frame}
      title={title}
      sandbox="allow-same-origin"
      srcDoc={css ? `<style>${css}</style>${html}` : html}
      onLoad={fit}
      className={cn("w-full rounded-md border-0", !css && "bg-white", fill && "h-full")}
      style={fill ? undefined : { height: minHeight }}
    />
  );
}

/** A Unix time (seconds) in the user's locale. */
export function formatTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString();
}

/** Bytes as KB, MB or GB. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

/** Turn whatever a Tauri command threw into a RedApiError. */
export function toApiError(err: unknown): RedApiError {
  if (err instanceof RedApiError) return err;
  if (typeof err === "object" && err !== null && "message" in err) {
    return new RedApiError(err as ConstructorParameters<typeof RedApiError>[0]);
  }
  return new RedApiError({ code: "internal", message: String(err) });
}
