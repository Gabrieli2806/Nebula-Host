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
import { activeRemote } from "@/lib/connection";
import { api, onAppEvent, onLocalAppEvent } from "@/lib/ipc";
import type { AppEvent, TransferPhase, TransferProgress } from "@/types/api";

export const PHASES: TransferPhase[] = ["prepare", "export", "upload", "verify", "adopt", "finalize"];

interface ActiveTransfer {
  transferId: string;
  /** Name of the machine it is going to, when this window started it. */
  target?: string;
  progress: TransferProgress | null;
  /** The person asked to call it off; waiting for it to stop. */
  cancelling?: boolean;
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

/** Ask for the transfer to be called off. It stops at its next safe point. */
export function cancelTransfer(instanceId: string) {
  const current = active.get(instanceId);
  if (!current || current.cancelling) return;
  publish(new Map(active).set(instanceId, { ...current, cancelling: true }));
  void api.transfer.cancel(current.transferId).catch(() => undefined);
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
      cancelling: current?.cancelling,
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
