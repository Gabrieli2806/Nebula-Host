/**
 * UI density, frontend-only like the theme.
 *
 * `<html data-density="compact">` swaps the compact token block in tokens.css.
 * Stored under the `nebula.*` prefix and applied before the first render (see
 * main.tsx), so the choice holds from the first paint after a restart. Not to
 * be confused with `AppSettings.density`, which is the server list's
 * cards/list switch.
 */

import { useCallback, useState } from "react";

export type UiDensity = "comfortable" | "compact";

const DENSITY_KEY = "nebula.uiDensity";

export function readDensity(): UiDensity {
  try {
    return localStorage.getItem(DENSITY_KEY) === "compact" ? "compact" : "comfortable";
  } catch {
    return "comfortable";
  }
}

export function applyDensity(density: UiDensity) {
  if (density === "compact") document.documentElement.dataset.density = "compact";
  else delete document.documentElement.dataset.density;
}

/** Call once before the first render. */
export function applyStoredDensity() {
  applyDensity(readDensity());
}

export function useUiDensity(): [UiDensity, (next: UiDensity) => void] {
  const [density, setDensity] = useState<UiDensity>(readDensity);
  const set = useCallback((next: UiDensity) => {
    setDensity(next);
    try {
      localStorage.setItem(DENSITY_KEY, next);
    } catch {
      // Storage blocked: the choice still holds for this run.
    }
    applyDensity(next);
  }, []);
  return [density, set];
}
