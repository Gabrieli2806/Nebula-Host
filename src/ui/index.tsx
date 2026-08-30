/**
 * The primitives every screen is built from.
 *
 * Small on purpose: a button, a panel, a field, a way to show nothing, and a
 * way to show a failure. Screens compose these rather than inventing their own
 * spacing and borders.
 */

import {
  useEffect,
  useRef,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";

import { useI18n } from "@/i18n";
import { toAppError, type AppError } from "@/lib/ipc";
import { Icon, type IconName } from "./Icon";

export { Icon } from "./Icon";
export type { IconName } from "./Icon";
export { StatusChip, StatusDot, statusHint, statusStyle } from "./StatusChip";

// -- button -----------------------------------------------------------------

type ButtonVariant = "default" | "primary" | "quiet" | "danger";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "default" | "sm";
  icon?: IconName;
  /** Renders as an icon-only button; `children` becomes the accessible name. */
  iconOnly?: boolean;
}

export function Button({
  variant = "default",
  size = "default",
  icon,
  iconOnly = false,
  className = "",
  children,
  ...props
}: ButtonProps) {
  const variantClass =
    variant === "primary"
      ? "btn-primary"
      : variant === "quiet"
        ? "btn-quiet"
        : variant === "danger"
          ? "btn-danger"
          : "";

  return (
    <button
      type="button"
      className={`btn ${variantClass} ${size === "sm" ? "btn-sm" : ""} ${
        iconOnly ? "btn-icon" : ""
      } ${className}`}
      aria-label={iconOnly && typeof children === "string" ? children : props["aria-label"]}
      {...props}
    >
      {icon ? <Icon name={icon} /> : null}
      {iconOnly ? null : children}
    </button>
  );
}

// -- panel ------------------------------------------------------------------

export function Panel({
  title,
  actions,
  children,
  className = "",
  bodyClassName = "",
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={`panel-flush flex min-h-0 flex-col ${className}`}>
      {title ? (
        <header className="panel-header">
          <h2 className="text-[length:var(--text-base)] font-semibold">{title}</h2>
          {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      <div className={`min-h-0 flex-1 ${bodyClassName}`}>{children}</div>
    </section>
  );
}

// -- fields -----------------------------------------------------------------

export function Field({
  label,
  help,
  error,
  children,
  hint,
}: {
  label: string;
  help?: string;
  error?: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="flex items-center justify-between gap-3">
        <span className="text-[length:var(--text-sm)] font-medium">{label}</span>
        {hint}
      </span>
      {children}
      {help && !error ? (
        <span className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">{help}</span>
      ) : null}
      {error ? (
        <span className="text-[length:var(--text-xs)] text-[color:var(--status-crashed)]">
          {error}
        </span>
      ) : null}
    </label>
  );
}

export function TextInput({ className = "", ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={`field ${className}`} {...props} />;
}

export function Select({ className = "", ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select className={`field ${className}`} {...props} />;
}

/** A switch that reads as a sentence, for settings written in plain language. */
export function Toggle({
  checked,
  onChange,
  label,
  help,
  disabled,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  help?: string;
  disabled?: boolean;
}) {
  return (
    <label className="flex cursor-pointer items-start gap-3 py-1">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className="no-drag relative mt-0.5 h-[18px] w-[32px] flex-none rounded-full border transition-colors"
        style={{
          background: checked ? "var(--accent)" : "var(--surface-sunken)",
          borderColor: checked ? "var(--accent)" : "var(--line-strong)",
          opacity: disabled ? 0.5 : 1,
        }}
      >
        <span
          className="absolute top-[2px] h-[12px] w-[12px] rounded-full transition-[left] duration-150"
          style={{
            left: checked ? "16px" : "2px",
            background: checked ? "var(--text-on-accent)" : "var(--text-muted)",
          }}
        />
      </button>
      <span className="flex flex-col gap-0.5">
        <span className="text-[length:var(--text-sm)]">{label}</span>
        {help ? (
          <span className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">{help}</span>
        ) : null}
      </span>
    </label>
  );
}

// -- feedback ---------------------------------------------------------------

export function EmptyState({
  icon,
  title,
  hint,
  action,
}: {
  icon?: IconName;
  title: string;
  hint?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 px-6 py-14 text-center">
      {icon ? (
        <span className="text-[color:var(--text-faint)]">
          <Icon name={icon} size={24} />
        </span>
      ) : null}
      <p className="text-[length:var(--text-lg)] font-medium">{title}</p>
      {hint ? (
        <p className="max-w-[46ch] text-[color:var(--text-muted)]">{hint}</p>
      ) : null}
      {action}
    </div>
  );
}

/**
 * The designed error surface: what happened, why, and what to do — with the
 * raw upstream text tucked behind a disclosure, where it belongs.
 */
export function ErrorNotice({
  error,
  onRetry,
  compact = false,
}: {
  error: unknown;
  onRetry?: () => void;
  compact?: boolean;
}) {
  const { t, describeError } = useI18n();
  const described = describeError(toAppError(error as AppError));

  return (
    <div
      className={`flex gap-3 rounded-[var(--radius)] border p-4 ${compact ? "items-center" : "items-start"}`}
      style={{ borderColor: "var(--status-crashed)", background: "var(--status-crashed-dim)" }}
      role="alert"
    >
      <span className="mt-0.5 flex-none text-[color:var(--status-crashed)]">
        <Icon name="warning" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="font-semibold">{described.title}</p>
        {!compact ? (
          <>
            <p className="text-[color:var(--text-muted)]">{described.cause}</p>
            <p>{described.action}</p>
            {described.detail ? (
              <details className="mt-1">
                <summary className="cursor-pointer text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                  {t("errors.details")}
                </summary>
                <pre className="selectable mono mt-2 max-h-40 overflow-auto whitespace-pre-wrap text-[length:var(--text-xs)] text-[color:var(--text-muted)]">
                  {described.detail}
                </pre>
              </details>
            ) : null}
          </>
        ) : null}
      </div>
      {onRetry ? (
        <Button size="sm" onClick={onRetry}>
          {t("actions.retry")}
        </Button>
      ) : null}
    </div>
  );
}

export function Skeleton({ className = "" }: { className?: string }) {
  return <div className={`skeleton ${className}`} aria-hidden="true" />;
}

// -- dialog -----------------------------------------------------------------

export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  width = 520,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    // Escape closes, and focus moves in — a dialog you cannot leave by
    // keyboard is a dialog that fails for anyone not using a mouse.
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    ref.current?.querySelector<HTMLElement>("input, select, textarea, button")?.focus();
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-6"
      style={{ background: "rgb(0 0 0 / 55%)" }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="panel-flush flex max-h-full w-full flex-col"
        style={{ maxWidth: width, boxShadow: "var(--shadow-pop)" }}
      >
        <header className="panel-header">
          <h2 className="text-[length:var(--text-lg)] font-semibold">{title}</h2>
          <Button variant="quiet" size="sm" iconOnly icon="close" onClick={onClose}>
            close
          </Button>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-5">{children}</div>
        {footer ? (
          <footer className="flex items-center justify-end gap-2 border-t border-[color:var(--line)] p-4">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

// -- tabs -------------------------------------------------------------------

export function Tabs<T extends string>({
  tabs,
  active,
  onChange,
}: {
  tabs: { id: T; label: string; icon?: IconName }[];
  active: T;
  onChange: (id: T) => void;
}) {
  return (
    <div role="tablist" className="flex items-center gap-1 overflow-x-auto">
      {tabs.map((tab) => {
        const selected = tab.id === active;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={selected}
            onClick={() => onChange(tab.id)}
            className="btn btn-quiet btn-sm"
            style={
              selected
                ? {
                    background: "var(--accent-dim)",
                    borderColor: "var(--accent-line)",
                    color: "var(--accent)",
                  }
                : undefined
            }
          >
            {tab.icon ? <Icon name={tab.icon} /> : null}
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}

// -- meter ------------------------------------------------------------------

export function Meter({
  value,
  max = 100,
  label,
  detail,
  colour = "var(--accent)",
}: {
  value: number;
  max?: number;
  label?: string;
  detail?: string;
  colour?: string;
}) {
  const percent = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  return (
    <div className="flex flex-col gap-1.5">
      {label || detail ? (
        <div className="flex items-baseline justify-between gap-3 text-[length:var(--text-xs)]">
          <span className="text-[color:var(--text-muted)]">{label}</span>
          <span className="mono text-[color:var(--text-muted)]">{detail}</span>
        </div>
      ) : null}
      <div
        className="meter"
        role="progressbar"
        aria-valuenow={Math.round(percent)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label}
      >
        <div className="meter-fill" style={{ width: `${percent}%`, background: colour }} />
      </div>
    </div>
  );
}

// -- ring -------------------------------------------------------------------

/**
 * A proportion as a dial. At 44px a ring reads faster than a bar and gives the
 * node panel something with craft in it — the one place the old app's dashboard
 * was better than a row of flat meters.
 */
export function Ring({
  value,
  max = 100,
  label,
  detail,
  size = 46,
  colour = "var(--accent)",
}: {
  value: number;
  max?: number;
  label: string;
  detail?: string;
  size?: number;
  colour?: string;
}) {
  const percent = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  const stroke = 3.5;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;

  return (
    <div className="flex flex-col items-center gap-1.5">
      <div className="relative" style={{ width: size, height: size }}>
        <svg
          width={size}
          height={size}
          role="progressbar"
          aria-label={label}
          aria-valuenow={Math.round(percent)}
          aria-valuemin={0}
          aria-valuemax={100}
          /* Start the arc at twelve o'clock. */
          style={{ transform: "rotate(-90deg)" }}
        >
          <circle
            className="ring-track"
            cx={size / 2}
            cy={size / 2}
            r={radius}
            strokeWidth={stroke}
          />
          <circle
            className="ring-value"
            cx={size / 2}
            cy={size / 2}
            r={radius}
            strokeWidth={stroke}
            stroke={colour}
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - percent / 100)}
          />
        </svg>
        <span className="mono absolute inset-0 flex items-center justify-center text-[length:var(--text-xs)]">
          {Math.round(percent)}%
        </span>
      </div>
      <span className="label leading-none">{label}</span>
      {detail ? (
        <span className="mono text-[length:var(--text-xs)] leading-none text-[color:var(--text-faint)]">
          {detail}
        </span>
      ) : null}
    </div>
  );
}
