/**
 * The primitives every screen is built from.
 *
 * Small on purpose: buttons, cards, wells, lamps, meters, a way to show
 * nothing and a way to show a failure. Screens compose these rather than inventing their
 * own spacing and borders. All state is expressed as attributes
 * (aria-checked, aria-selected, data-*), so the look is CSS-only
 * (src/styles/global.css).
 */

import {
  useEffect,
  useId,
  useRef,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";

import { useI18n } from "@/i18n";
import { toAppError, type AppError } from "@/lib/ipc";
import { Icon, type IconName } from "./Icon";

export { Icon } from "./Icon";
export type { IconName } from "./Icon";
export {
  StatusChip,
  StatusDot,
  statusHint,
  statusStyle,
  isActive,
  isIdle,
} from "./StatusChip";
export { Mark } from "./Mark";
export { CopyAddress } from "./CopyAddress";
export { ServerTile } from "./ServerTile";
export { LoaderIcon, DistributionTag } from "./LoaderIcon";
export type { LoaderGlyph } from "./LoaderIcon";
export { identityProps, initialOf } from "./identity";
export { PlayerHead } from "./PlayerHead";
export { playerHeadUrl, isOfflineUuid } from "./playerHeadUrl";
export type { PlayerHeadSize } from "./playerHeadUrl";

// -- button -----------------------------------------------------------------

type ButtonVariant = "default" | "primary" | "quiet" | "danger";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** default: secondary (navy, hairline). primary: solid accent, one per
   *  view. quiet: borderless (dense rows, toolbars). danger: red outline. */
  variant?: ButtonVariant;
  size?: "default" | "sm";
  icon?: IconName;
  /** Renders as an icon-only button; `children` becomes the accessible name. */
  iconOnly?: boolean;
}

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  default: "",
  primary: "btn-primary",
  quiet: "btn-quiet",
  danger: "btn-danger",
};

export function Button({
  variant = "default",
  size = "default",
  icon,
  iconOnly = false,
  className = "",
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      type="button"
      className={`btn ${VARIANT_CLASS[variant]} ${size === "sm" ? "btn-sm" : ""} ${
        iconOnly ? "btn-icon" : ""
      } ${className}`}
      aria-label={iconOnly && typeof children === "string" ? children : props["aria-label"]}
      title={iconOnly && typeof children === "string" ? children : props.title}
      {...props}
    >
      {icon ? <Icon name={icon} /> : null}
      {iconOnly ? null : children}
    </button>
  );
}

// -- panel ------------------------------------------------------------------

/** A card with an optional header (title + actions) and a body. */
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
    <section className={`panel-flush panel-stack ${className}`}>
      {title || actions ? (
        <header className="panel-header">
          {title ? <h2 className="panel-title min-w-0">{title}</h2> : null}
          {actions ? <div className="ml-auto flex flex-none items-center gap-2">{actions}</div> : null}
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
        <span className="text-[length:var(--text-sm)] font-[600]">{label}</span>
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

/**
 * A bare switch: button[role=switch].toggle, state in aria-checked, knob drawn
 * by CSS. Use it inside your own row layout (Settings preference rows).
 */
export function Switch({
  checked,
  onChange,
  label,
  disabled,
  className = "",
  id,
  describedBy,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  /** Accessible name. */
  label: string;
  disabled?: boolean;
  className?: string;
  id?: string;
  /** Id of the help or consequence text, read after the name. */
  describedBy?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-label={label}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`toggle no-drag ${className}`}
    />
  );
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
  const helpId = useId();
  return (
    <label className="flex cursor-pointer items-start gap-3 py-1">
      <Switch
        checked={checked}
        onChange={onChange}
        label={label}
        disabled={disabled}
        describedBy={help ? helpId : undefined}
        className="mt-0.5"
      />
      <span className="flex flex-col gap-0.5">
        <span className="text-[length:var(--text-sm)]">{label}</span>
        {help ? (
          <span id={helpId} className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">
            {help}
          </span>
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
  className = "",
}: {
  icon?: IconName;
  title: string;
  hint?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={`empty-state ${className}`}>
      {icon ? (
        <span className="empty-state-icon">
          <Icon name={icon} size={24} />
        </span>
      ) : null}
      <p className="empty-state-title">{title}</p>
      {hint ? <p className="empty-state-hint">{hint}</p> : null}
      {action ? <div className="mt-1">{action}</div> : null}
    </div>
  );
}

/**
 * The designed error surface: what happened, why, and what to do, with the
 * raw upstream text tucked behind a disclosure, where it belongs.
 * `compact` shows the title only (inline under a card's address).
 */
export function ErrorNotice({
  error,
  onRetry,
  compact = false,
  className = "",
}: {
  error: unknown;
  onRetry?: () => void;
  compact?: boolean;
  className?: string;
}) {
  const { t, describeError } = useI18n();
  const described = describeError(toAppError(error as AppError));

  return (
    <div className={`notice ${className}`} data-compact={compact ? "" : undefined} role="alert">
      <span className="notice-icon">
        <Icon name="warning" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className={`font-[650] ${compact ? "text-[length:var(--text-sm)]" : ""}`}>
          {described.title}
        </p>
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
  header,
  width = 520,
  dismissable = true,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  /** Extra content under the title row (e.g. a step header). */
  header?: ReactNode;
  width?: number;
  /**
   * False while something runs that cannot be called off: no close key,
   * and Escape or a click outside do nothing.
   */
  dismissable?: boolean;
}) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const footerRef = useRef<HTMLElement>(null);
  const titleId = useId();

  // Most callers pass a fresh onClose every render. Holding it in a ref keeps
  // the open effect (and its focus move) to once per opening; otherwise every
  // keystroke in a field would throw focus somewhere else.
  const onCloseRef = useRef(onClose);
  const dismissableRef = useRef(dismissable);
  useEffect(() => {
    onCloseRef.current = onClose;
    dismissableRef.current = dismissable;
  }, [onClose, dismissable]);

  useEffect(() => {
    if (!open) return;
    // Escape closes, and focus moves in: a dialog you cannot leave by
    // keyboard is a dialog that fails for anyone not using a mouse.
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && dismissableRef.current) onCloseRef.current();
    };
    document.addEventListener("keydown", onKey);
    // First text field or select in the body; else the first footer key
    // (Cancel, in a confirm); else the close key.
    const target =
      bodyRef.current?.querySelector<HTMLElement>(
        "input:not([type=checkbox]):not([type=radio]):not(:disabled), select:not(:disabled), textarea:not(:disabled)",
      ) ??
      footerRef.current?.querySelector<HTMLElement>("button:not(:disabled)") ??
      ref.current?.querySelector<HTMLElement>("button");
    target?.focus();
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  return (
    <div
      className="dialog-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && dismissable) onClose();
      }}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="dialog"
        style={{ maxWidth: width }}
      >
        <header className="flex flex-col gap-3 border-b border-[color:var(--line)] px-5 pb-4 pt-5">
          <div className="flex items-start justify-between gap-3">
            <h2 id={titleId} className="dialog-title">
              {title}
            </h2>
            {dismissable ? (
              <Button variant="quiet" size="sm" iconOnly icon="close" onClick={onClose} className="-mr-2 -mt-1">
                {t("dialog.close")}
              </Button>
            ) : null}
          </div>
          {header}
        </header>
        <div ref={bodyRef} className="min-h-0 flex-1 overflow-auto p-5">
          {children}
        </div>
        {footer ? (
          <footer ref={footerRef} className="dialog-footer">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

// -- tabs & segmented -------------------------------------------------------

/** Arrow-key movement shared by tabs and radio segments. */
function moveFocus(event: KeyboardEvent<HTMLDivElement>) {
  if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
  const items = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"),
  );
  const index = items.indexOf(document.activeElement as HTMLButtonElement);
  if (index < 0) return;
  event.preventDefault();
  const next = items[(index + (event.key === "ArrowRight" ? 1 : -1) + items.length) % items.length];
  next?.focus();
  next?.click();
}

export function Tabs<T extends string>({
  tabs,
  active,
  onChange,
  label,
  size = "default",
  className = "",
}: {
  tabs: { id: T; label: string; icon?: IconName }[];
  active: T;
  onChange: (id: T) => void;
  /** Accessible name for the tab list. */
  label?: string;
  size?: "default" | "sm";
  className?: string;
}) {
  return (
    <div
      role="tablist"
      aria-label={label}
      className={`segmented ${className}`}
      data-size={size === "sm" ? "sm" : undefined}
      onKeyDown={moveFocus}
    >
      {tabs.map((tab) => {
        const selected = tab.id === active;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(tab.id)}
            className="segment"
          >
            {tab.icon ? <Icon name={tab.icon} /> : null}
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * One-of-n segments in a well (radiogroup): view switch, theme, density.
 * Same look as Tabs; use this when it is a setting, Tabs when it swaps a view.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  size = "default",
  className = "",
}: {
  options: { id: T; label: string; icon?: IconName; iconOnly?: boolean }[];
  value: T;
  onChange: (id: T) => void;
  /** Accessible name for the group. */
  label: string;
  size?: "default" | "sm";
  className?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={`segmented no-drag ${className}`}
      data-size={size === "sm" ? "sm" : undefined}
      onKeyDown={moveFocus}
    >
      {options.map((option) => {
        const selected = option.id === value;
        return (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.iconOnly ? option.label : undefined}
            title={option.iconOnly ? option.label : undefined}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(option.id)}
            className="segment"
          >
            {option.icon ? <Icon name={option.icon} /> : null}
            {option.iconOnly ? null : option.label}
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
  colour,
}: {
  value: number;
  max?: number;
  label?: string;
  detail?: string;
  /** Fill colour; defaults to the accent (from CSS). */
  colour?: string;
}) {
  const percent = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  return (
    <div className="flex flex-col gap-1.5">
      {label || detail ? (
        <div className="flex items-baseline justify-between gap-3 text-[length:var(--text-xs)]">
          <span className="text-[color:var(--text-muted)]">{label}</span>
          <span className="readout text-[color:var(--text-muted)]">{detail}</span>
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
        <div
          className="meter-fill"
          style={colour ? { width: `${percent}%`, background: colour } : { width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

// -- ring -------------------------------------------------------------------

/**
 * The machine meter (classic Nebula): a thin accent arc on a hairline track,
 * the percentage inside in the mono face, the label (and an optional detail
 * such as "of 32 GB") underneath.
 *
 *   <Ring label={t("node.cpu")} value={health.cpu_percent} />
 *   <Ring label={t("node.memory")} value={used} max={total} detail="of 32 GB" />
 */
export function Ring({
  value,
  max = 100,
  label,
  detail,
  size = 56,
  stroke = 3,
  colour,
  showValue = true,
  className = "",
}: {
  value: number;
  max?: number;
  /** Visible label under the dial, and the accessible name. */
  label: string;
  detail?: string;
  /** Dial diameter in px (default 56). */
  size?: number;
  /** Arc width in px (default 3). */
  stroke?: number;
  /** Arc colour; defaults to the accent (from CSS). */
  colour?: string;
  /** Show the rounded percentage inside the dial. */
  showValue?: boolean;
  className?: string;
}) {
  const percent = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const centre = size / 2;

  return (
    <div className={`meter-ring ${className}`}>
      <div className="meter-ring-dial" style={{ width: size, height: size }}>
        <svg
          width={size}
          height={size}
          viewBox={`0 0 ${size} ${size}`}
          role="meter"
          aria-label={label}
          aria-valuenow={Math.round(percent)}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <circle className="meter-ring-track" cx={centre} cy={centre} r={radius} strokeWidth={stroke} />
          <circle
            className="meter-ring-value"
            cx={centre}
            cy={centre}
            r={radius}
            strokeWidth={stroke}
            style={colour ? { stroke: colour } : undefined}
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - percent / 100)}
          />
        </svg>
        {showValue ? (
          <span className="meter-ring-readout" aria-hidden="true">
            {Math.round(percent)}%
          </span>
        ) : null}
      </div>
      <span className="meter-ring-label">{label}</span>
      {detail ? <span className="meter-ring-detail">{detail}</span> : null}
    </div>
  );
}
