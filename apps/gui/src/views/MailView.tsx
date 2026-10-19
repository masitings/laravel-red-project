import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { DownloadIcon, InboxIcon, PaperclipIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import type { MailMessage } from "@/api/gen/MailMessage";
import { useMail, useMailActions, useMailMessage, useStatus } from "@/api/queries";
import {
  CodeBlock,
  Empty,
  ErrorNote,
  formatTime,
  HtmlFrame,
  PageHeader,
  toApiError,
} from "@/components/common";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

async function saveFile(path: string, defaultPath: string) {
  const destination = await save({ defaultPath });
  if (destination) {
    try {
      await invoke("save_mail_file", { path, destination });
    } catch (err) {
      throw toApiError(err);
    }
  }
}

export function MailView() {
  const mail = useMail();
  const actions = useMailActions();
  const status = useStatus();
  const [selected, setSelected] = useState<string | null>(null);
  const list = mail.data ?? [];
  const current = list.find((m) => m.id === selected) ? selected : null;

  return (
    <div className="space-y-6">
      <PageHeader
        title="Mail"
        description="Everything your sites send. RED catches it all; nothing is delivered."
        actions={
          list.length > 0 && (
            <Button
              variant="outline"
              onClick={() =>
                actions.clear.mutate(undefined, { onSuccess: () => setSelected(null) })
              }
            >
              <Trash2Icon /> Delete all
            </Button>
          )
        }
      />
      <ErrorNote error={mail.error ?? actions.clear.error ?? actions.remove.error} />
      {mail.data && list.length === 0 && (
        <Empty icon={<InboxIcon />}>
          <p>No mail yet. Point your app at RED&apos;s mail catcher:</p>
          <CodeBlock className="text-left">
            {`MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=${status.data?.smtp_port ?? 2525}\nMAIL_USERNAME=\${APP_NAME}`}
          </CodeBlock>
        </Empty>
      )}
      {list.length > 0 && (
        <Card className="py-0">
          <CardContent className="grid min-h-[30rem] grid-cols-[minmax(15rem,1fr)_2fr] px-0">
            <ul className="max-h-[75vh] divide-y overflow-y-auto border-r text-sm">
              {list.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    onClick={() => setSelected(m.id)}
                    aria-current={current === m.id ? "true" : undefined}
                    className={cn(
                      "w-full space-y-0.5 px-3 py-2.5 text-left transition-colors",
                      current === m.id ? "bg-accent" : "hover:bg-accent/50",
                    )}
                  >
                    <div className="flex items-center gap-2">
                      {!m.read && (
                        <>
                          <span className="size-2 shrink-0 rounded-full bg-primary" />
                          <span className="sr-only">unread</span>
                        </>
                      )}
                      <span className={cn("truncate", !m.read && "font-semibold")}>
                        {m.subject || "(no subject)"}
                      </span>
                    </div>
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <span className="truncate">{m.to.join(", ")}</span>
                      {m.mailbox && <Badge variant="secondary">{m.mailbox}</Badge>}
                    </div>
                    <div className="text-xs text-muted-foreground/80">{formatTime(m.received)}</div>
                  </button>
                </li>
              ))}
            </ul>
            <div className="min-w-0 p-4">
              {current ? (
                <Message
                  id={current}
                  onDelete={() =>
                    actions.remove.mutate(current, { onSuccess: () => setSelected(null) })
                  }
                />
              ) : (
                <p className="text-sm text-muted-foreground">Select a message.</p>
              )}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function Message({ id, onDelete }: { id: string; onDelete: () => void }) {
  const message = useMailMessage(id);
  const [saveError, setSaveError] = useState<unknown>(null);
  const m: MailMessage | undefined = message.data;
  if (message.error) return <ErrorNote error={message.error} />;
  if (!m) return <p className="text-sm text-muted-foreground">Loading…</p>;

  const download = (path: string, name: string) => {
    setSaveError(null);
    saveFile(path, name).catch(setSaveError);
  };

  return (
    <div className="flex h-full flex-col gap-3 text-sm">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-0.5">
          <h2 className="text-base font-semibold">{m.summary.subject || "(no subject)"}</h2>
          <div className="text-muted-foreground">From {m.summary.from}</div>
          <div className="text-muted-foreground">
            To {m.summary.to.join(", ")}
            {m.cc.length > 0 && ` · Cc ${m.cc.join(", ")}`}
          </div>
          <div className="text-xs text-muted-foreground/80">{formatTime(m.summary.received)}</div>
        </div>
        <div className="flex shrink-0 gap-1">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => download(`/v1/mail/${id}/raw`, `${id}.eml`)}
          >
            <DownloadIcon /> Save .eml
          </Button>
          <Button size="sm" variant="ghost" onClick={onDelete}>
            <Trash2Icon /> Delete
          </Button>
        </div>
      </div>
      <ErrorNote error={saveError} />
      {m.attachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {m.attachments.map((a) => (
            <Button
              key={a.index}
              size="sm"
              variant="outline"
              onClick={() => download(`/v1/mail/${id}/attachments/${a.index}`, a.filename)}
            >
              <PaperclipIcon /> {a.filename} · {Math.max(1, Math.round(a.size / 1024))} KB
            </Button>
          ))}
        </div>
      )}
      <Tabs defaultValue={m.html ? "html" : "text"} className="min-h-[22rem] flex-1">
        <TabsList>
          <TabsTrigger value="html" disabled={!m.html}>
            HTML
          </TabsTrigger>
          <TabsTrigger value="text">Text</TabsTrigger>
          <TabsTrigger value="headers">Headers</TabsTrigger>
        </TabsList>
        <TabsContent value="html" className="min-h-[20rem]">
          {m.html && <HtmlFrame title="Message" html={m.html} fill />}
        </TabsContent>
        <TabsContent value="text">
          <pre className="font-sans whitespace-pre-wrap">{m.text ?? "(no plain-text part)"}</pre>
        </TabsContent>
        <TabsContent value="headers">
          <dl className="grid grid-cols-[10rem_1fr] gap-x-3 gap-y-1 text-xs">
            {m.headers.map((h, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: headers never reorder, and names repeat
              <div key={`${h.name}-${i}`} className="contents">
                <dt className="font-medium text-muted-foreground">{h.name}</dt>
                <dd className="font-mono break-all">{h.value}</dd>
              </div>
            ))}
            <dt className="font-medium text-muted-foreground">Envelope to</dt>
            <dd className="font-mono break-all">{m.envelope_to.join(", ")}</dd>
          </dl>
        </TabsContent>
      </Tabs>
    </div>
  );
}
