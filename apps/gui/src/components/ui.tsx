import type { ButtonHTMLAttributes, ReactNode } from "react";
import { RedApiError } from "../api/client";

export function Button({
  variant = "secondary",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "secondary" | "ghost" }) {
  const styles = {
    primary: "bg-red-600 text-white hover:bg-red-700 disabled:bg-red-300",
    secondary:
      "border border-stone-300 bg-white hover:bg-stone-100 dark:border-stone-700 dark:bg-stone-900 dark:hover:bg-stone-800",
    ghost: "hover:bg-stone-100 dark:hover:bg-stone-800",
  }[variant];
  return (
    <button
      type="button"
      className={`rounded-md px-2.5 py-1 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${styles} ${className}`}
      {...props}
    />
  );
}

export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "good" | "warn" | "bad";
}) {
  const styles = {
    neutral: "bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300",
    good: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300",
    warn: "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300",
    bad: "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300",
  }[tone];
  return <span className={`rounded px-1.5 py-0.5 text-xs font-medium ${styles}`}>{children}</span>;
}

export function Card({
  title,
  actions,
  children,
}: {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
      <header className="flex items-center justify-between gap-3 border-b border-stone-200 px-4 py-3 dark:border-stone-800">
        <h2 className="font-medium">{title}</h2>
        {actions && <div className="flex gap-2">{actions}</div>}
      </header>
      <div className="p-4">{children}</div>
    </section>
  );
}

/** Shows a failed request with the daemon's hint, if any. */
export function ErrorNote({ error }: { error: unknown }) {
  if (!error) return null;
  const message = error instanceof Error ? error.message : String(error);
  const hint = error instanceof RedApiError ? error.hint : undefined;
  return (
    <p role="alert" className="text-sm text-red-700 dark:text-red-400">
      {message}
      {hint && <span className="text-stone-500"> — {hint}</span>}
    </p>
  );
}
