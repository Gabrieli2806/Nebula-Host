/**
 * The game's agreement (the Minecraft EULA), asked for when it is needed.
 *
 * The backend refuses to start (or put to sleep) a server whose operator has
 * not accepted the agreement, with `invalid_input` / `agreement_required`.
 * The lifecycle actions catch that one error and raise a request here;
 * `AgreementGate` (mounted once, in App) shows it and, once the user ticks
 * the box, records the acceptance and repeats what they asked for.
 *
 * Nothing in the app accepts on the user's behalf: the only path to
 * `instances.acceptAgreement` is a person ticking a checkbox.
 */

import { useSyncExternalStore } from "react";

import { isAppError } from "./ipc";

export interface AgreementRequest {
  instanceId: string;
  /** The full text, from the backend (the manifest's `agreement.url`). */
  url: string;
  /** What the user was doing, repeated once they accept. */
  resume: "start" | "sleep";
}

let current: AgreementRequest | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

export function requestAgreement(request: AgreementRequest) {
  current = request;
  emit();
}

export function clearAgreementRequest() {
  current = null;
  emit();
}

export function useAgreementRequest(): AgreementRequest | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
    () => null,
  );
}

/** The agreement an error asks for, when it is that error. */
export function agreementRequired(error: unknown): { url: string } | null {
  if (!isAppError(error)) return null;
  if (error.code !== "invalid_input" || error.params?.reason !== "agreement_required") return null;
  return { url: error.params.url ?? "" };
}
