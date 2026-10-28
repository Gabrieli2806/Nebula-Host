/**
 * The account the launcher plays with. Several can be signed in; one is
 * primary, chosen from the account menu, and remembered on this device.
 */

import { useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";

import { launcherApi } from "@/lib/ipc";
import type { LauncherAccount } from "./types";
import { launcherKeys } from "./hooks";

const KEY = "nebula.launcherAccount";
const EVENT = "nebula:launcher-account";

function read(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function setPrimaryAccount(id: string) {
  try {
    localStorage.setItem(KEY, id);
  } catch {
    // Storage blocked: the choice lasts until reload.
  }
  window.dispatchEvent(new Event(EVENT));
}

function subscribe(onChange: () => void) {
  window.addEventListener(EVENT, onChange);
  return () => window.removeEventListener(EVENT, onChange);
}

export function usePrimaryAccount(): { accounts: LauncherAccount[]; primary: LauncherAccount | null; loading: boolean } {
  const chosen = useSyncExternalStore(subscribe, read, read);
  const query = useQuery({ queryKey: launcherKeys.accounts, queryFn: launcherApi.accounts, refetchInterval: 60_000 });
  const accounts = query.data ?? [];
  const primary = accounts.find((a) => a.id === chosen) ?? accounts[0] ?? null;
  return { accounts, primary, loading: query.isPending };
}
