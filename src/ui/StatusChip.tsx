/**
 * The one status component.
 *
 * Six states, one place. Every list, header and card renders status through
 * this — the previous app re-implemented it as ternary chains in four files
 * and they drifted apart.
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

/** Statuses that are going somewhere, and say so with a pulse. */
const IN_FLIGHT: ReadonlySet<InstanceStatus> = new Set<InstanceStatus>(["starting", "stopping"]);

export function statusStyle(status: InstanceStatus): CSSProperties {
  const token = TOKENS[status];
  return { "--chip-fg": token.fg, "--chip-bg": token.bg } as CSSProperties;
}

export function StatusChip({ status, className = "" }: { status: InstanceStatus; className?: string }) {
  const { t } = useI18n();
  return (
    <span className={`status-chip ${className}`} style={statusStyle(status)}>
      <StatusDot status={status} />
      {t(`status.${status}`)}
    </span>
  );
}

/** The dot on its own, for dense rows where the word does not fit. */
export function StatusDot({ status, size = 7 }: { status: InstanceStatus; size?: number }) {
  const pulsing = IN_FLIGHT.has(status);
  return (
    <span className="relative inline-flex" style={statusStyle(status)}>
      <span
        className={`status-dot ${pulsing ? "status-dot-pulse" : ""}`}
        style={{ width: size, height: size }}
      />
    </span>
  );
}

/** The line under a status that says what it means for the player. */
export function statusHint(status: InstanceStatus): string | null {
  if (status === "sleeping") return "status.sleepingHint";
  if (status === "starting") return "status.startingHint";
  return null;
}
