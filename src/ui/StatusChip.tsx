/**
 * The one status component.
 *
 * Six states, one place. Every list, header and card renders status through
 * this. Status is a pilot lamp plus its word, never colour alone: the lamp
 * also carries a shape (see `.status-dot[data-status]` in global.css), so an
 * accent can never be mistaken for a state.
 */

import type { CSSProperties } from "react";

import { useI18n } from "@/i18n";
import type { InstanceStatus } from "@/types/api";

const TOKENS: Record<InstanceStatus, { fg: string; bg: string }> = {
  offline: { fg: "var(--status-offline)", bg: "var(--status-offline-dim)" },
  sleeping: { fg: "var(--status-sleeping)", bg: "var(--status-sleeping-dim)" },
  starting: { fg: "var(--status-starting)", bg: "var(--status-starting-dim)" },
  online: { fg: "var(--status-online)", bg: "var(--status-online-dim)" },
  stopping: { fg: "var(--status-stopping)", bg: "var(--status-stopping-dim)" },
  crashed: { fg: "var(--status-crashed)", bg: "var(--status-crashed-dim)" },
};

/** Statuses where the server process is up, or on its way up or down. */
const ACTIVE: ReadonlySet<InstanceStatus> = new Set<InstanceStatus>(["online", "starting", "stopping"]);

/**
 * Active vs idle. Active (online / starting / stopping) shows the full
 * identity colour; idle (offline / sleeping) is slightly desaturated and its
 * name may step back to --text-muted. Crashed is neither: full identity,
 * crashed lamp.
 */
export function isActive(status: InstanceStatus): boolean {
  return ACTIVE.has(status);
}

export function isIdle(status: InstanceStatus): boolean {
  return status === "offline" || status === "sleeping";
}


/**
 * `--chip-fg` / `--chip-bg` for a status, for any surface that wants to speak
 * in the status colour (the overview status word, a 3px status rule).
 */
export function statusStyle(status: InstanceStatus): CSSProperties {
  const token = TOKENS[status];
  return { "--chip-fg": token.fg, "--chip-bg": token.bg } as CSSProperties;
}

export function StatusChip({ status, className = "" }: { status: InstanceStatus; className?: string }) {
  const { t } = useI18n();
  return (
    <span className={`status-chip ${className}`} data-status={status} style={statusStyle(status)}>
      <StatusDot status={status} />
      {t(`status.${status}`)}
    </span>
  );
}

/**
 * The lamp on its own, for dense rows. Put the status word next to it
 * anyway (visually or as `label` for screen readers): never colour alone.
 */
export function StatusDot({
  status,
  size = 8,
  label,
}: {
  status: InstanceStatus;
  size?: number;
  /** Accessible name when the word is not shown next to the lamp. */
  label?: string;
}) {
  const style = { ...statusStyle(status), "--lamp-size": `${size}px` } as CSSProperties;
  return (
    <span
      className="relative inline-flex flex-none"
      style={style}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <span className="status-dot" data-status={status} />
    </span>
  );
}

/** The line under a status that says what it means for the player. */
export function statusHint(status: InstanceStatus): string | null {
  if (status === "sleeping") return "status.sleepingHint";
  if (status === "starting") return "status.startingHint";
  if (status === "crashed") return "overview.crashedHint";
  return null;
}
