/**
 * The theme preference: System, Dark or Light.
 *
 * Frontend only. The choice lives in localStorage under `nebula.theme`. The
 * effective theme is written to `<html data-theme>`, which is
 * all tokens.css looks at. "system" follows `prefers-color-scheme` live.
 *
 * A tiny external store, so ThemeSync and the Settings control share one
 * source of truth without a context provider.
 */

import { useSyncExternalStore } from "react";

export const THEME_PREFERENCES = ["system", "dark", "light"] as const;
export type ThemePreference = (typeof THEME_PREFERENCES)[number];
export type EffectiveTheme = "dark" | "light";

const STORAGE_KEY = "nebula.theme";
const LIGHT_QUERY = "(prefers-color-scheme: light)";

export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (THEME_PREFERENCES as readonly string[]).includes(value);
}

function readStored(): ThemePreference {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return isThemePreference(value) ? value : "system";
  } catch {
    return "system";
  }
}

let current: ThemePreference = readStored();
const listeners = new Set<() => void>();

function systemPrefersLight(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(LIGHT_QUERY).matches
    : false;
}

/** What a preference means right now ("system" asks the OS). */
export function resolveTheme(preference: ThemePreference = current): EffectiveTheme {
  if (preference === "system") return systemPrefersLight() ? "light" : "dark";
  return preference;
}

/** Writes the effective theme onto <html>. */
export function applyTheme(preference: ThemePreference = current): void {
  document.documentElement.dataset.theme = resolveTheme(preference);
}

/** Call once before the first render (main.tsx) to avoid a flash. */
export function applyStoredTheme(): void {
  current = readStored();
  applyTheme(current);
}

export function getThemePreference(): ThemePreference {
  return current;
}

export function setThemePreference(next: ThemePreference): void {
  current = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Storage blocked: the choice still holds for this run.
  }
  applyTheme(next);
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  const media = typeof window.matchMedia === "function" ? window.matchMedia(LIGHT_QUERY) : null;
  const onMedia = () => listener();
  const onStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY) return;
    current = readStored();
    listener();
  };
  media?.addEventListener("change", onMedia);
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    media?.removeEventListener("change", onMedia);
    window.removeEventListener("storage", onStorage);
  };
}

/**
 * The stored preference and a setter, for the Settings > Appearance control.
 *
 *   const [theme, setTheme] = useThemePreference();
 */
export function useThemePreference(): [ThemePreference, (next: ThemePreference) => void] {
  const preference = useSyncExternalStore(subscribe, getThemePreference, () => "system" as const);
  return [preference, setThemePreference];
}

/** The theme actually showing ("dark" | "light"), live with the OS. */
export function useEffectiveTheme(): EffectiveTheme {
  return useSyncExternalStore(
    subscribe,
    () => resolveTheme(current),
    () => "dark" as const,
  );
}
