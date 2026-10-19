/**
 * Transfers in progress, wherever they are looked at from.
 *
 * The transfer dialog can be hidden while it runs, and the server's card (and
 * its overview) keep showing what is happening: which machine it is going to
 * and which step it is on. The dialog says when it starts; progress events
 * from this PC and from the machine being managed keep it current.
 */

import { useEffect, useSyncExternalStore } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { activeRemote } from "@/lib/connection";
import { formatBytes } from "@/lib/format";
import { onAppEvent, onLocalAppEvent } from "@/lib/ipc";
import type { AppEvent, TransferPhase, TransferProgress } from "@/types/api";
import { Icon } from "@/ui";

import "./transfer.css";

export const PHASES: TransferPhase[] = ["prepare", "export", "upload", "verify", "adopt", "finalize"];

interface ActiveTransfer {
  transferId: string;
  /** Name of the machine it is going to, when this window started it. */
  target?: string;
  progress: TransferProgress | null;
}

let active = new Map<string, ActiveTransfer>();
const listeners = new Set<() => void>();

function publish(next: Map<string, ActiveTransfer>) {
  active = next;
  for (const listener of listeners) listener();
}

export function beginTransfer(instanceId: string, transferId: string, target: string) {
  publish(new Map(active).set(instanceId, { transferId, target, progress: null }));
}

export function endTransfer(instanceId: string) {
  if (!active.has(instanceId)) return;
  const next = new Map(active);
  next.delete(instanceId);
  publish(next);
}

export function applyTransferProgress(instanceId: string, progress: TransferProgress) {
  const current = active.get(instanceId);
  if (progress.failed || progress.done) {
    // Finished one way or the other: the instance record now says which.
    if (!current || current.transferId === progress.transferId) endTransfer(instanceId);
    return;
  }
  if (current && current.transferId !== progress.transferId) return;
  const before = current?.progress;
  // The two channels can interleave; never step back to an earlier phase.
  if (before && PHASES.indexOf(progress.phase) < PHASES.indexOf(before.phase)) return;
  publish(
    new Map(active).set(instanceId, {
      transferId: progress.transferId,
      target: current?.target,
      progress,
    }),
  );
}

export function useActiveTransfer(instanceId: string): ActiveTransfer | undefined {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => active.get(instanceId),
    () => undefined,
  );
}

/** Mounted once at the app root: feeds the store from both event channels. */
export function useTransferTracking() {
  const client = useQueryClient();
  useEffect(() => {
    const take = (event: AppEvent) => {
      if (event.type !== "transfer") return;
      applyTransferProgress(event.instanceId, event.progress);
      if (event.progress.done || event.progress.failed) {
        void client.invalidateQueries({ queryKey: keys.instances });
      }
    };
    const stops = [onLocalAppEvent(take)];
    if (activeRemote()) stops.push(onAppEvent(take));
    return () => stops.forEach((stop) => stop());
  }, [client]);
}

/** The strip on a card or overview: where it is going and which step. */
export function TransferProgressStrip({ instanceId, compact = false }: { instanceId: string; compact?: boolean }) {
  const { t } = useI18n();
  const transfer = useActiveTransfer(instanceId);
  if (!transfer) return null;

  const target = transfer.target ?? t("transfer.otherMachine");
  const progress = transfer.progress;
  const hasBytes = progress !== null && progress.total > 0;
  const share = hasBytes ? Math.min(100, Math.round((progress.bytes / progress.total) * 100)) : null;
  const step = progress ? PHASES.indexOf(progress.phase) + 1 : 1;
  const phase = t(`transfer.phase.${progress?.phase ?? "prepare"}`, { target });

  return (
    <div className="transfer-strip" data-compact={compact ? "" : undefined} role="status" aria-live="polite">
      <div className="transfer-strip-head">
        <Icon name="send" size={13} />
        <span className="transfer-strip-title">{t("transfer.running", { target })}</span>
        <span className="transfer-strip-step readout">
          {step}/{PHASES.length}
        </span>
      </div>
      <div className="transfer-strip-phase">
        <span>{phase}</span>
        {hasBytes ? (
          <span className="readout">
            {compact ? `${share}%` : `${formatBytes(progress.bytes)} / ${formatBytes(progress.total)}`}
          </span>
        ) : null}
      </div>
      <span className="transfer-strip-bar" aria-hidden="true" data-indeterminate={share === null ? "" : undefined}>
        <span style={share === null ? undefined : { width: `${share}%` }} />
      </span>
    </div>
  );
}

/** The compact version for a table row: a tag with the share sent. */
export function TransferTag({ instanceId }: { instanceId: string }) {
  const { t } = useI18n();
  const transfer = useActiveTransfer(instanceId);
  if (!transfer) return null;
  const progress = transfer.progress;
  const share =
    progress && progress.total > 0 ? ` ${Math.min(100, Math.round((progress.bytes / progress.total) * 100))}%` : "";
  return (
    <span className="tag transfer-tag" role="status">
      <Icon name="send" size={11} />
      {t("transfer.tagRunning")}
      {share}
    </span>
  );
}
