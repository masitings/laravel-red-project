import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import type { MailMessage } from "../api/gen/MailMessage";
import { useMail, useMailActions, useMailMessage, useStatus } from "../api/queries";
import { Badge, Button, Card, ErrorNote, formatTime, HtmlFrame } from "../components/ui";

type Pane = "html" | "text" | "headers";

async function saveFile(path: string, defaultPath: string) {
  const destination = await save({ defaultPath });
  if (destination) await invoke("save_mail_file", { path, destination });
}

export function MailView() {
  const mail = useMail();
  const actions = useMailActions();
  const status = useStatus();
  const [selected, setSelected] = useState<string | null>(null);
  const list = mail.data ?? [];
  const current = list.find((m) => m.id === selected) ? selected : null;

  return (
    <Card
      title="Mail"
      actions={
        list.length > 0 && (
          <Button
            variant="ghost"
            onClick={() => actions.clear.mutate(undefined, { onSuccess: () => setSelected(null) })}
          >
            Delete all
          </Button>
        )
      }
    >
      <ErrorNote error={mail.error ?? actions.clear.error ?? actions.remove.error} />
      {mail.data && list.length === 0 && (
        <div className="text-sm text-stone-500">
          <p>No mail yet. Point your app at RED's mail catcher:</p>
          <pre className="mt-2 select-all rounded bg-stone-100 p-2 text-xs dark:bg-stone-800">
            {`MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=${status.data?.smtp_port ?? 2525}\nMAIL_USERNAME=\${APP_NAME}`}
          </pre>
        </div>
      )}
      {list.length > 0 && (
        <div className="grid min-h-[28rem] grid-cols-[minmax(14rem,1fr)_2fr] gap-4">
          <ul className="max-h-[70vh] divide-y divide-stone-100 overflow-y-auto text-sm dark:divide-stone-800">
            {list.map((m) => (
              <li key={m.id}>
                <button
                  type="button"
                  onClick={() => setSelected(m.id)}
                  aria-current={current === m.id ? "true" : undefined}
                  className={`w-full px-2 py-2 text-left ${
                    current === m.id
                      ? "bg-stone-100 dark:bg-stone-800"
                      : "hover:bg-stone-50 dark:hover:bg-stone-900"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    {!m.read && (
                      <>
                        <span className="size-2 shrink-0 rounded-full bg-red-600" />
                        <span className="sr-only">unread</span>
                      </>
                    )}
                    <span className={`truncate ${m.read ? "" : "font-semibold"}`}>
                      {m.subject || "(no subject)"}
                    </span>
                  </div>
                  <div className="flex items-center gap-2 text-xs text-stone-500">
                    <span className="truncate">{m.to.join(", ")}</span>
                    {m.mailbox && <Badge>{m.mailbox}</Badge>}
                  </div>
                  <div className="text-xs text-stone-400">{formatTime(m.received)}</div>
                </button>
              </li>
            ))}
          </ul>
          <div className="min-w-0">
            {current ? (
              <Message
                id={current}
                onDelete={() =>
                  actions.remove.mutate(current, { onSuccess: () => setSelected(null) })
                }
              />
            ) : (
              <p className="text-sm text-stone-500">Select a message.</p>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}

function Message({ id, onDelete }: { id: string; onDelete: () => void }) {
  const message = useMailMessage(id);
  const [pane, setPane] = useState<Pane>("html");
  const [saveError, setSaveError] = useState<unknown>(null);
  const m: MailMessage | undefined = message.data;
  if (message.error) return <ErrorNote error={message.error} />;
  if (!m) return <p className="text-sm text-stone-500">Loading…</p>;

  const shown: Pane = pane === "html" && !m.html ? "text" : pane;
  const download = (path: string, name: string) => {
    setSaveError(null);
    saveFile(path, name).catch(setSaveError);
  };

  return (
    <div className="flex h-full flex-col gap-3 text-sm">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-0.5">
          <h3 className="text-base font-medium">{m.summary.subject || "(no subject)"}</h3>
          <div className="text-stone-600 dark:text-stone-400">From {m.summary.from}</div>
          <div className="text-stone-600 dark:text-stone-400">
            To {m.summary.to.join(", ")}
            {m.cc.length > 0 && ` · Cc ${m.cc.join(", ")}`}
          </div>
          <div className="text-xs text-stone-400">{formatTime(m.summary.received)}</div>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button variant="ghost" onClick={() => download(`/v1/mail/${id}/raw`, `${id}.eml`)}>
            Save .eml
          </Button>
          <Button variant="ghost" onClick={onDelete}>
            Delete
          </Button>
        </div>
      </div>
      <ErrorNote error={saveError} />
      {m.attachments.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {m.attachments.map((a) => (
            <Button
              key={a.index}
              variant="secondary"
              onClick={() => download(`/v1/mail/${id}/attachments/${a.index}`, a.filename)}
            >
              {a.filename} · {Math.max(1, Math.round(a.size / 1024))} KB
            </Button>
          ))}
        </div>
      )}
      <div className="flex gap-1 border-b border-stone-200 dark:border-stone-800" role="tablist">
        {(["html", "text", "headers"] as const).map((p) => (
          <button
            key={p}
            type="button"
            role="tab"
            aria-selected={shown === p}
            disabled={p === "html" && !m.html}
            onClick={() => setPane(p)}
            className={`px-3 py-1.5 text-xs uppercase tracking-wide disabled:opacity-40 ${
              shown === p ? "border-b-2 border-red-600 font-medium" : "text-stone-500"
            }`}
          >
            {p}
          </button>
        ))}
      </div>
      <div className="min-h-[20rem] flex-1">
        {shown === "html" && m.html && <HtmlFrame title="Message" html={m.html} fill />}
        {shown === "text" && (
          <pre className="whitespace-pre-wrap font-sans">{m.text ?? "(no plain-text part)"}</pre>
        )}
        {shown === "headers" && (
          <dl className="grid grid-cols-[10rem_1fr] gap-x-3 gap-y-1 text-xs">
            {m.headers.map((h, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: headers never reorder, and names repeat
              <div key={`${h.name}-${i}`} className="contents">
                <dt className="font-medium text-stone-500">{h.name}</dt>
                <dd className="break-all font-mono">{h.value}</dd>
              </div>
            ))}
            <dt className="font-medium text-stone-500">Envelope to</dt>
            <dd className="break-all font-mono">{m.envelope_to.join(", ")}</dd>
          </dl>
        )}
      </div>
    </div>
  );
}
