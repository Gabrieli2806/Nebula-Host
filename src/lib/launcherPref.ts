/**
 * Whether this device shows the Minecraft launcher. It belongs to the device
 * (accounts and instances are local), so it lives in this browser's storage,
 * not in the managed machine's settings. On by default on phones, where it is
 * the reason to install; off on PCs, where Nebula Host is a server host first.
 */

import { useSyncExternalStore } from "react";

import { IS_MOBILE, IS_WEB } from "@/lib/web";

const KEY = "nebula.launcherEnabled";
const EVENT = "nebula:launcher-pref";

function read(): boolean {
  if (IS_WEB) return false;
  try {
    const stored = localStorage.getItem(KEY);
    if (stored !== null) return stored === "1";
  } catch {
    // Storage blocked: fall back to the platform default.
  }
  return IS_MOBILE;
}

export function setLauncherEnabled(enabled: boolean) {
  try {
    localStorage.setItem(KEY, enabled ? "1" : "0");
  } catch {
    // Storage blocked: the choice lasts until reload.
  }
  window.dispatchEvent(new Event(EVENT));
}

function subscribe(onChange: () => void) {
  window.addEventListener(EVENT, onChange);
  window.addEventListener("storage", onChange);
  return () => {
    window.removeEventListener(EVENT, onChange);
    window.removeEventListener("storage", onChange);
  };
}

export function useLauncherEnabled(): boolean {
  return useSyncExternalStore(subscribe, read, read);
}
